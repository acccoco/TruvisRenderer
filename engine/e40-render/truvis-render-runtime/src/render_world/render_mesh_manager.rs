use std::collections::{HashSet, VecDeque};
use std::mem::size_of_val;
use std::ptr;

use anyhow::Result;
use ash::vk;
use slotmap::SecondaryMap;

use truvis_asset::handle::{MeshData, SubmeshData};
use truvis_gfx::commands::barrier::{GfxBarrierMask, GfxBufferBarrier};
use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_gfx::commands::command_pool::GfxCommandPool;
use truvis_gfx::commands::semaphore::GfxSemaphore;
use truvis_gfx::commands::submit_info::GfxSubmitInfo;
use truvis_gfx::gfx::{GfxDeviceCtx, GfxQueueCtx, GfxResourceCtx};
use truvis_gfx::raytracing::acceleration::GfxAcceleration;
use truvis_gfx::resources::buffer::GfxBuffer;
use truvis_gfx::resources::layout::GfxVertexLayout;
use truvis_gfx::resources::lifecycle::DestroyReason;
use truvis_gfx::resources::special_buffers::acceleration_buffer::GfxAccelerationScratchBuffer;
use truvis_gfx::resources::special_buffers::index_buffer::GfxIndex32Buffer;
use truvis_gfx::resources::special_buffers::vertex_buffer::GfxVertexBuffer;
use truvis_gfx::resources::vertex_layout::soa_3d::VertexLayoutSoA3D;
use truvis_world::guid_new_type::MeshHandle;
use truvis_world::SceneReadView;
use truvis_render_foundation::frame_label::FrameLabel;

use crate::render_world::geometry::{RtGeometry, RtTriangleMeta};
use crate::render_world::render_data::MeshRenderData;
use crate::render_world::render_resolver::MeshRenderResolver;
/// 已提交到 graphics queue、但尚未确认完成的 mesh 上传任务。
///
/// 这里同时持有 staging/scratch/geometry/BLAS owner，是因为 timeline 到达前这些资源
/// 都仍可能被 copy 或 acceleration build 命令引用，不能交给 resolver 或提前释放。
struct SubmittedMeshUpload {
    semaphore_value: u64,
    handle: MeshHandle,
    command_buffer: GfxCommandBuffer,
    staging_buffers: Vec<GfxBuffer>,
    scratch_buffer: GfxAccelerationScratchBuffer,
    geometries: Vec<RtGeometry>,
    triangle_metadata: Vec<Vec<RtTriangleMeta>>,
    blas: GfxAcceleration,
    name: String,
}

/// 已通过 timeline 检测确认完成的 mesh GPU 资源。
///
/// `RenderMeshManager` 接管该结构后，mesh 才会进入 `meshes` map，供 instance bridge
/// 解析为 render-side 几何数据。
struct FinishedMeshUpload {
    handle: MeshHandle,
    geometries: Vec<RtGeometry>,
    triangle_metadata: Vec<Vec<RtTriangleMeta>>,
    blas: GfxAcceleration,
    name: String,
}

/// Mesh GPU 上传和 BLAS build 队列。
///
/// 只在渲染线程使用。它在 graphics queue 上提交 vertex/index buffer copy 和 BLAS build，
/// 因为 acceleration structure build 不应假设 transfer queue 支持。
/// 完成检测同样通过 timeline semaphore 异步推进，避免帧循环在资产加载期间被 GPU 上传阻塞。
struct MeshUploadQueue {
    command_pool: Option<GfxCommandPool>,
    timeline_semaphore: Option<GfxSemaphore>,
    next_timeline_value: u64,
    pending_uploads: VecDeque<SubmittedMeshUpload>,
    destroyed: bool,
}

impl MeshUploadQueue {
    /// 创建只服务 mesh 上传和 BLAS build 的 graphics command pool 与 timeline。
    ///
    /// 这里绑定 graphics queue family，而不是 transfer queue family，是因为 BLAS build 属于
    /// acceleration structure 命令，不能依赖独立 transfer queue 支持。
    fn new(device_ctx: GfxDeviceCtx<'_>, queue_ctx: GfxQueueCtx<'_>) -> Self {
        let command_pool = GfxCommandPool::new(
            device_ctx,
            queue_ctx.gfx_queue().queue_family().clone(),
            vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER,
            "AssetMeshUploadPool",
        );
        let timeline_semaphore = GfxSemaphore::new_timeline(device_ctx, 0, "AssetMeshUploadTimeline");

        Self {
            command_pool: Some(command_pool),
            timeline_semaphore: Some(timeline_semaphore),
            next_timeline_value: 1,
            pending_uploads: VecDeque::new(),
            destroyed: false,
        }
    }

    fn submit_mesh_upload(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        queue_ctx: GfxQueueCtx<'_>,
        handle: MeshHandle,
        data: &MeshData,
    ) -> Result<()> {
        let _span = tracy_client::span!("MeshUploadQueue::submit_mesh_upload");
        let name = data.name.clone();
        let mut geometries = Vec::with_capacity(data.submeshes.len());
        let mut triangle_metadata = Vec::with_capacity(data.submeshes.len());
        let mut staging_buffers = Vec::with_capacity(data.submeshes.len() * 2);

        for (submesh_index, submesh) in data.submeshes.iter().enumerate() {
            // CPU submesh 数据在这里被转换为 render-side SoA 顶点布局与 index buffer。
            // 一个 submesh 对应 BLAS 内一条 geometry，多个 geometry 共享 mesh 级 BLAS 生命周期。
            let vertex_count = submesh.positions.len();
            let index_count = submesh.indices.len();
            let submesh_name =
                if submesh.name.is_empty() { format!("{name}-submesh{submesh_index}") } else { submesh.name.clone() };

            let vertex_buffer = GfxVertexBuffer::<VertexLayoutSoA3D>::new_device_local(
                resource_ctx,
                vertex_count,
                format!("{submesh_name}-vertex"),
            );
            let index_buffer =
                GfxIndex32Buffer::new_device_local(resource_ctx, index_count, format!("{submesh_name}-index"));
            let vertex_stage_buffer = Self::create_vertex_stage_buffer(
                resource_ctx,
                vertex_count,
                submesh,
                format!("{submesh_name}-vertex-stage"),
            );
            let index_stage_buffer =
                Self::create_index_stage_buffer(resource_ctx, &submesh.indices, format!("{submesh_name}-index-stage"));

            geometries.push(RtGeometry {
                vertex_buffer,
                index_buffer,
            });
            triangle_metadata.push(Self::build_triangle_metadata(submesh));
            staging_buffers.push(vertex_stage_buffer);
            staging_buffers.push(index_stage_buffer);
        }

        // BLAS 输入直接引用刚创建的 device-local vertex/index buffer。后续 command buffer
        // 会先完成 staging copy，再通过 barrier 保证 build 命令读取到复制后的内容。
        let blas_inputs = geometries.iter().map(RtGeometry::get_blas_geometry_info).collect::<Vec<_>>();
        let (blas, scratch_buffer) = GfxAcceleration::new_blas_for_build(
            resource_ctx,
            device_ctx,
            &blas_inputs,
            vk::BuildAccelerationStructureFlagsKHR::empty(),
            &name,
        );

        let command_pool = self.command_pool.as_ref().expect("MeshUploadQueue used after shutdown");
        let timeline_semaphore = self.timeline_semaphore.as_ref().expect("MeshUploadQueue used after shutdown");
        let command_buffer = GfxCommandBuffer::new(device_ctx, command_pool, "AssetMeshUploadCmd");

        command_buffer.begin(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT, "AssetMeshUpload");
        for (geometry, stage_pair) in geometries.iter().zip(staging_buffers.chunks_exact(2)) {
            let vertex_stage_buffer = &stage_pair[0];
            let index_stage_buffer = &stage_pair[1];
            command_buffer.cmd_copy_buffer(
                vertex_stage_buffer,
                &geometry.vertex_buffer,
                &[vk::BufferCopy {
                    size: vertex_stage_buffer.size(),
                    ..Default::default()
                }],
            );
            command_buffer.cmd_copy_buffer(
                index_stage_buffer,
                &geometry.index_buffer,
                &[vk::BufferCopy {
                    size: index_stage_buffer.size(),
                    ..Default::default()
                }],
            );
        }

        // vertex/index copy 与 BLAS build 在同一个 graphics command buffer 中录制。
        // Vulkan 验证层会把 BLAS 的 device-address 输入视为 shader read，因此 barrier 同时覆盖
        // ACCELERATION_STRUCTURE_READ 和 SHADER_READ，防止 copy 后立即 build 的同步漏洞。
        let transfer_to_blas_mask = GfxBarrierMask {
            src_stage: vk::PipelineStageFlags2::TRANSFER,
            src_access: vk::AccessFlags2::TRANSFER_WRITE,
            dst_stage: vk::PipelineStageFlags2::ACCELERATION_STRUCTURE_BUILD_KHR,
            // BLAS build 通过 device address 读取 vertex/index 输入，验证层按 shader read 归类。
            dst_access: vk::AccessFlags2::ACCELERATION_STRUCTURE_READ_KHR | vk::AccessFlags2::SHADER_READ,
        };
        let mut barriers = Vec::with_capacity(geometries.len() * 2);
        for geometry in &geometries {
            barriers.push(GfxBufferBarrier::default().mask(transfer_to_blas_mask).buffer(
                geometry.vertex_buffer.vk_buffer(),
                0,
                vk::WHOLE_SIZE,
            ));
            barriers.push(GfxBufferBarrier::default().mask(transfer_to_blas_mask).buffer(
                geometry.index_buffer.vk_buffer(),
                0,
                vk::WHOLE_SIZE,
            ));
        }
        command_buffer.buffer_memory_barrier(vk::DependencyFlags::empty(), &barriers);

        let blas_geometries = blas_inputs.iter().map(|blas_input| blas_input.geometry).collect::<Vec<_>>();
        let range_infos = blas_inputs.iter().map(|blas_input| blas_input.range).collect::<Vec<_>>();
        let build_geometry_info = vk::AccelerationStructureBuildGeometryInfoKHR::default()
            .ty(vk::AccelerationStructureTypeKHR::BOTTOM_LEVEL)
            .flags(vk::BuildAccelerationStructureFlagsKHR::PREFER_FAST_TRACE)
            .geometries(&blas_geometries)
            .mode(vk::BuildAccelerationStructureModeKHR::BUILD)
            .dst_acceleration_structure(blas.handle())
            .scratch_data(vk::DeviceOrHostAddressKHR {
                device_address: scratch_buffer.device_address(),
            });
        command_buffer.build_acceleration_structure(&build_geometry_info, &range_infos);
        // BLAS build 完成后马上建立 read barrier，保证后续 TLAS build 或 ray tracing shader
        // 读取同一个 BLAS handle/device address 时能看到完整加速结构内容。
        command_buffer.memory_barrier(&[vk::MemoryBarrier2 {
            src_stage_mask: vk::PipelineStageFlags2::ACCELERATION_STRUCTURE_BUILD_KHR,
            src_access_mask: vk::AccessFlags2::ACCELERATION_STRUCTURE_WRITE_KHR,
            dst_stage_mask: vk::PipelineStageFlags2::ACCELERATION_STRUCTURE_BUILD_KHR
                | vk::PipelineStageFlags2::RAY_TRACING_SHADER_KHR,
            dst_access_mask: vk::AccessFlags2::ACCELERATION_STRUCTURE_READ_KHR | vk::AccessFlags2::SHADER_READ,
            ..Default::default()
        }]);
        command_buffer.end();

        let target_value = self.next_timeline_value;
        self.next_timeline_value += 1;
        // 每个 mesh upload 对应一个 timeline value；完成前 geometry/BLAS 都保留在 pending 队列，
        // 只有完成后才进入 resolver 可见的 `meshes` map。
        let submit_info = GfxSubmitInfo::new(std::slice::from_ref(&command_buffer)).signal(
            timeline_semaphore,
            vk::PipelineStageFlags2::ALL_COMMANDS,
            Some(target_value),
        );
        queue_ctx.gfx_queue().submit(vec![submit_info], None);

        log::trace!(
            "RenderMeshManager: submitted mesh {:?} '{}' submeshes={} timeline={}",
            handle,
            name,
            geometries.len(),
            target_value
        );
        self.pending_uploads.push_back(SubmittedMeshUpload {
            semaphore_value: target_value,
            handle,
            command_buffer,
            staging_buffers,
            scratch_buffer,
            geometries,
            triangle_metadata,
            blas,
            name,
        });

        Ok(())
    }

    /// 非阻塞推进上传队列，并返回已经 GPU-ready 的 mesh。
    ///
    /// 队列按提交顺序 signal 单调递增 timeline value，因此只要队首未完成，后续任务也一定
    /// 还不能对 resolver 可见。
    fn update(&mut self, resource_ctx: GfxResourceCtx<'_>, device_ctx: GfxDeviceCtx<'_>) -> Vec<FinishedMeshUpload> {
        let _span = tracy_client::span!("MeshUploadQueue::update");
        let device = device_ctx.device();
        let timeline_semaphore = self.timeline_semaphore.as_ref().expect("MeshUploadQueue used after shutdown");
        let command_pool = self.command_pool.as_ref().expect("MeshUploadQueue used after shutdown");
        let current_value = unsafe { device.get_semaphore_counter_value(timeline_semaphore.handle()).unwrap_or(0) };

        let mut finished_uploads = Vec::new();
        while let Some(upload) = self.pending_uploads.front() {
            if current_value < upload.semaphore_value {
                break;
            }

            // staging 和 scratch 只服务本次上传/build，timeline 到达后即可释放；
            // geometry 与 BLAS 则转交给 RenderMeshManager，成为 render pass 可解析的数据。
            let upload = self.pending_uploads.pop_front().unwrap();
            command_pool.free_command_buffers(device_ctx, vec![upload.command_buffer]);
            for staging_buffer in upload.staging_buffers {
                staging_buffer.destroy(resource_ctx, DestroyReason::DeferredCleanup);
            }
            upload.scratch_buffer.destroy(resource_ctx, DestroyReason::DeferredCleanup);
            finished_uploads.push(FinishedMeshUpload {
                handle: upload.handle,
                geometries: upload.geometries,
                triangle_metadata: upload.triangle_metadata,
                blas: upload.blas,
                name: upload.name,
            });
        }

        finished_uploads
    }

    /// 关闭上传队列并释放仍未交给 `RenderMeshManager` 的 pending 资源。
    ///
    /// shutdown 路径允许等待 timeline，因为此时帧循环已经停止；等待完成后才能销毁
    /// command buffer、staging、scratch、geometry 和 BLAS。
    fn shutdown(&mut self, resource_ctx: GfxResourceCtx<'_>, device_ctx: GfxDeviceCtx<'_>) {
        if self.destroyed {
            return;
        }

        let Some(timeline_semaphore) = self.timeline_semaphore.take() else {
            self.destroyed = true;
            return;
        };
        let mut command_pool = self.command_pool.take().expect("MeshUploadQueue command pool missing");

        if let Some(last_upload) = self.pending_uploads.back() {
            const WAIT_SEMAPHORE_TIMEOUT_NS: u64 = 30 * 1000 * 1000 * 1000;
            timeline_semaphore.wait_timeline(device_ctx, last_upload.semaphore_value, WAIT_SEMAPHORE_TIMEOUT_NS);
        }

        // shutdown 时 pending 队列中的 geometry/BLAS 尚未进入 uploaded cache，
        // 因此必须在等待 timeline 后由 upload queue 自己销毁。
        while let Some(upload) = self.pending_uploads.pop_front() {
            command_pool.free_command_buffers(device_ctx, vec![upload.command_buffer]);
            for staging_buffer in upload.staging_buffers {
                staging_buffer.destroy(resource_ctx, DestroyReason::Shutdown);
            }
            upload.scratch_buffer.destroy(resource_ctx, DestroyReason::Shutdown);
            for geometry in upload.geometries {
                geometry.destroy(resource_ctx, DestroyReason::Shutdown);
            }
            upload.blas.destroy(resource_ctx, device_ctx, DestroyReason::Shutdown);
        }

        timeline_semaphore.destroy(device_ctx);
        command_pool.destroy(device_ctx);
        self.destroyed = true;
    }

    fn create_vertex_stage_buffer(
        resource_ctx: GfxResourceCtx<'_>,
        vertex_count: usize,
        data: &SubmeshData,
        debug_name: impl AsRef<str>,
    ) -> GfxBuffer {
        let total_size = VertexLayoutSoA3D::buffer_size(vertex_count) as vk::DeviceSize;
        let stage_buffer = GfxBuffer::new_stage_buffer(resource_ctx, total_size, debug_name);
        // `VertexLayoutSoA3D` 要求 positions/normals/tangents/uvs 以 SoA 方式连续摆放。
        // 上面的 validate 已保证所有属性长度一致，因此这里可以按布局 offset 直接拷贝。
        unsafe {
            ptr::copy_nonoverlapping(
                data.positions.as_ptr() as *const u8,
                stage_buffer.mapped_ptr().add(VertexLayoutSoA3D::pos_offset(vertex_count) as usize),
                size_of_val(data.positions.as_slice()),
            );
            ptr::copy_nonoverlapping(
                data.normals.as_ptr() as *const u8,
                stage_buffer.mapped_ptr().add(VertexLayoutSoA3D::normal_offset(vertex_count) as usize),
                size_of_val(data.normals.as_slice()),
            );
            ptr::copy_nonoverlapping(
                data.tangents.as_ptr() as *const u8,
                stage_buffer.mapped_ptr().add(VertexLayoutSoA3D::tangent_offset(vertex_count) as usize),
                size_of_val(data.tangents.as_slice()),
            );
            ptr::copy_nonoverlapping(
                data.uvs.as_ptr() as *const u8,
                stage_buffer.mapped_ptr().add(VertexLayoutSoA3D::uv_offset(vertex_count) as usize),
                size_of_val(data.uvs.as_slice()),
            );
        }
        stage_buffer.flush(resource_ctx, 0, total_size);
        stage_buffer
    }

    fn create_index_stage_buffer(
        resource_ctx: GfxResourceCtx<'_>,
        indices: &[u32],
        debug_name: impl AsRef<str>,
    ) -> GfxBuffer {
        // index buffer 不需要额外重排，直接写入 staging 后由上传命令复制到 device-local buffer。
        let stage_buffer =
            GfxBuffer::new_stage_buffer(resource_ctx, size_of_val(indices) as vk::DeviceSize, debug_name);
        stage_buffer.transfer_data_by_mmap(resource_ctx, indices);
        stage_buffer
    }

    /// 从 upload-ready CPU mesh 中保留 light table 需要的最小三角形信息。
    ///
    /// 该函数和 vertex/index 上传同处 `MeshUploadQueue`，保证 CPU metadata 与真正提交给
    /// GPU 的索引顺序完全一致；后续 scene sync 只通过 mesh manager 的 ready cache 读取它，
    /// 不回到 asset hub 重新查询或复制整份 mesh。
    fn build_triangle_metadata(data: &SubmeshData) -> Vec<RtTriangleMeta> {
        data.indices
            .chunks_exact(3)
            .enumerate()
            .map(|(primitive_id, tri)| {
                let i0 = tri[0] as usize;
                let i1 = tri[1] as usize;
                let i2 = tri[2] as usize;
                let p0 = data.positions[i0];
                let p1 = data.positions[i1];
                let p2 = data.positions[i2];
                let local_area = 0.5 * (p1 - p0).cross(p2 - p0).length();
                RtTriangleMeta {
                    positions: [p0, p1, p2],
                    uvs: [data.uvs[i0], data.uvs[i1], data.uvs[i2]],
                    primitive_id: primitive_id as u32,
                    local_area,
                }
            })
            .collect()
    }
}

impl Drop for MeshUploadQueue {
    fn drop(&mut self) {
        debug_assert!(self.destroyed, "MeshUploadQueue dropped without explicit shutdown");
    }
}

/// resolver 可见的 GPU-ready mesh 缓存。
///
/// `geometries` 服务光栅化 draw 和 BLAS geometry 输入，`blas`/`blas_device_address`
/// 服务 TLAS 构建；二者共享同一批 vertex/index buffer，避免 mesh 在 runtime 内出现两套 GPU 表示。
struct UploadedMesh {
    geometries: Vec<RtGeometry>,
    triangle_metadata: Vec<Vec<RtTriangleMeta>>,
    blas: GfxAcceleration,
    blas_device_address: vk::DeviceAddress,
}

struct RetiredMesh {
    handle: MeshHandle,
    mesh: UploadedMesh,
    retired_frame_id: u64,
}

impl UploadedMesh {
    fn destroy(self, resource_ctx: GfxResourceCtx<'_>, device_ctx: GfxDeviceCtx<'_>, reason: DestroyReason) {
        for geometry in self.geometries {
            geometry.destroy(resource_ctx, reason);
        }
        self.blas.destroy(resource_ctx, device_ctx, reason);
    }
}

/// 渲染侧 mesh 资产上传与 BLAS 缓存。
///
/// 它把 `MeshHandle` 解析为光栅化和 ray tracing 共用的 GPU 几何数据。
pub struct RenderMeshManager {
    meshes: SecondaryMap<MeshHandle, UploadedMesh>,
    pending_meshes: HashSet<MeshHandle>,
    retired_resources: Vec<RetiredMesh>,
    upload_queue: MeshUploadQueue,
    current_frame_id: u64,
}

/// mesh 上传阶段对资源对账暴露的结构化结果。
impl RenderMeshManager {
    pub(crate) fn begin_frame(&mut self, current_frame_id: u64) {
        self.current_frame_id = current_frame_id;
    }

    /// 创建 mesh 管理器。
    ///
    /// 内部 command pool 绑定 graphics queue family，因为 BLAS build 不能假设 transfer queue 支持。
    pub fn new(device_ctx: GfxDeviceCtx<'_>, queue_ctx: GfxQueueCtx<'_>, current_frame_id: u64) -> Self {
        Self {
            meshes: SecondaryMap::new(),
            pending_meshes: HashSet::new(),
            retired_resources: Vec::new(),
            upload_queue: MeshUploadQueue::new(device_ctx, queue_ctx),
            current_frame_id,
        }
    }

    /// 对账 CPU mesh registry，并推进 GPU 上传/BLAS build 完成检测。
    ///
    /// 该方法只查询 graphics queue timeline semaphore，不等待 GPU；完成前 mesh 不会进入 resolver
    /// 可见的 `meshes` map，因此 instance bridge 会继续把依赖它的实例保持为 pending。
    pub fn sync_scene(
        &mut self,
        scene: SceneReadView<'_>,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        queue_ctx: GfxQueueCtx<'_>,
    ) {
        let _span = tracy_client::span!("RenderMeshManager::sync_scene");
        self.reclaim_retired_resources(resource_ctx, device_ctx);

        for handle in scene.mesh_handles() {
            if !self.needs_upload(handle) {
                continue;
            }
            let Some(data) = scene.mesh_data(handle) else {
                continue;
            };
            match self
                .upload_queue
                .submit_mesh_upload(resource_ctx, device_ctx, queue_ctx, handle, data)
            {
                Ok(()) => {
                    self.pending_meshes.insert(handle);
                }
                Err(err) => {
                    log::error!("Failed to submit mesh upload {:?}: {}", handle, err);
                }
            }
        }

        for finished in self.upload_queue.update(resource_ctx, device_ctx) {
            self.pending_meshes.remove(&finished.handle);
            if !scene.contains_mesh(finished.handle) {
                for geometry in finished.geometries {
                    geometry.destroy(resource_ctx, DestroyReason::DeferredCleanup);
                }
                finished.blas.destroy(resource_ctx, device_ctx, DestroyReason::DeferredCleanup);
                continue;
            }
            if self.meshes.contains_key(finished.handle) {
                log::error!("RenderMeshManager: reject duplicate upload for immutable mesh {:?}", finished.handle);
                for geometry in finished.geometries {
                    geometry.destroy(resource_ctx, DestroyReason::DeferredCleanup);
                }
                finished.blas.destroy(resource_ctx, device_ctx, DestroyReason::DeferredCleanup);
                continue;
            }
            self.install_uploaded_mesh(device_ctx, finished);
        }
    }

    /// 移除 scene mesh 对应的 GPU-ready cache。
    ///
    /// 已提交的上传/BLAS build 只能等待 timeline 自然完成；completion 会再次检查 CPU registry
    /// 中的 generational handle，不会把已删除 mesh 重新发布。
    pub fn remove_meshes(
        &mut self,
        handles: &[MeshHandle],
    ) {
        for &handle in handles {
            let Some(mesh) = self.meshes.remove(handle) else {
                continue;
            };
            self.retired_resources.push(RetiredMesh {
                handle,
                mesh,
                retired_frame_id: self.current_frame_id,
            });
        }
    }

    pub(crate) fn needs_upload(&self, handle: MeshHandle) -> bool {
        !self.meshes.contains_key(handle) && !self.pending_meshes.contains(&handle)
    }

    /// 按 CPU registry 的完整 membership 清理已经删除的 render-side mesh。
    ///
    /// 首期 mesh 内容不可变；这里仅处理资源生命周期，不支持同 handle 的内容替换。
    pub fn remove_stale_meshes(
        &mut self,
        scene: SceneReadView<'_>,
    ) -> bool {
        let live_handles = scene.mesh_handles().collect::<HashSet<_>>();
        let stale_handles = self
            .meshes
            .keys()
            .filter(|handle| !live_handles.contains(handle))
            .collect::<Vec<_>>();
        if stale_handles.is_empty() {
            return false;
        }

        self.remove_meshes(&stale_handles);
        true
    }

    fn install_uploaded_mesh(
        &mut self,
        device_ctx: GfxDeviceCtx<'_>,
        finished: FinishedMeshUpload,
    ) {
        let blas_device_address = finished.blas.device_address(device_ctx);
        // 缓存 BLAS device address，后续构建 TLAS 时无需重新查询 Vulkan handle。
        log::trace!(
            "RenderMeshManager: mesh {:?} '{}' is GPU ready, blas_address={:#x}",
            finished.handle,
            finished.name,
            blas_device_address
        );
        self.meshes.insert(
            finished.handle,
            UploadedMesh {
                geometries: finished.geometries,
                triangle_metadata: finished.triangle_metadata,
                blas: finished.blas,
                blas_device_address,
            },
        );
    }

    fn reclaim_retired_resources(&mut self, resource_ctx: GfxResourceCtx<'_>, device_ctx: GfxDeviceCtx<'_>) {
        let current_frame_id = self.current_frame_id;
        let fif_count = FrameLabel::COUNT as u64;
        let mut retained = Vec::new();
        for retired in self.retired_resources.drain(..) {
            if current_frame_id.saturating_sub(retired.retired_frame_id) >= fif_count {
                log::debug!("RenderMeshManager: reclaimed mesh {:?} after FIF window", retired.handle);
                retired.mesh.destroy(resource_ctx, device_ctx, DestroyReason::DeferredCleanup);
            } else {
                retained.push(retired);
            }
        }
        self.retired_resources = retained;
    }

    /// 关闭上传队列并释放所有 mesh GPU 资源。
    ///
    /// pending 队列会先等待对应 timeline value，确保 staging/scratch/geometry/BLAS 不再被 graphics queue 引用。
    pub fn destroy(mut self, resource_ctx: GfxResourceCtx<'_>, device_ctx: GfxDeviceCtx<'_>) {
        self.upload_queue.shutdown(resource_ctx, device_ctx);

        for (_, mesh) in self.meshes.drain() {
            mesh.destroy(resource_ctx, device_ctx, DestroyReason::Shutdown);
        }
        for retired in self.retired_resources.drain(..) {
            retired.mesh.destroy(resource_ctx, device_ctx, DestroyReason::Shutdown);
        }
    }
}

impl MeshRenderResolver for RenderMeshManager {
    fn resolve_mesh(&self, handle: MeshHandle) -> Option<MeshRenderData<'_>> {
        let mesh = self.meshes.get(handle)?;
        Some(MeshRenderData {
            geometries: mesh.geometries.as_slice(),
            triangle_metadata: mesh.triangle_metadata.as_slice(),
            blas_device_address: Some(mesh.blas_device_address),
        })
    }
}
