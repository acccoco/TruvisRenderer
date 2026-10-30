use std::collections::{HashSet, VecDeque};
use std::mem::size_of_val;
use std::ptr;

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
use truvis_render_foundation::frame_label::FrameLabel;
use truvis_world::SceneReadView;
use truvis_world::guid_new_type::MeshAssetHandle;

use crate::render_world::geometry::{RtGeometry, RtTriangleMeta};
use crate::render_world::render_data::MeshRenderData;
use crate::render_world::render_resolver::MeshRenderResolver;
/// 已提交到 graphics queue、但尚未确认完成的 mesh 上传任务。
///
/// 这里同时持有 staging/scratch/geometry/BLAS owner，是因为 timeline 到达前这些资源
/// 都仍可能被 copy 或 acceleration build 命令引用，不能交给 resolver 或提前释放。
struct SubmittedMeshUpload {
    semaphore_value: u64,
    handle: MeshAssetHandle,
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
/// `GpuMeshStore` 接管该结构后，mesh 才会进入 `meshes` map，供 instance bridge
/// 解析为 render-side 几何数据。
struct FinishedMeshUpload {
    handle: MeshAssetHandle,
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
        let timeline_semaphore = { GfxSemaphore::new_timeline(device_ctx, 0, "AssetMeshUploadTimeline") };

        Self {
            command_pool: Some(command_pool),
            timeline_semaphore: Some(timeline_semaphore),
            next_timeline_value: 1,
            pending_uploads: VecDeque::new(),

            destroyed: false,
        }
    }

    /// 沿用底层资源创建与队列提交的 fail-fast 契约；正常返回只表示已入队，不表示 GPU-ready。
    /// 输入来自已验证的 CPU mesh registry；准备异常撤销本次资源，提交异常保留 pending owner。
    fn submit_mesh_upload(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        queue_ctx: GfxQueueCtx<'_>,
        handle: MeshAssetHandle,
        data: &MeshData,
    ) {
        let _span = tracy_client::span!("MeshUploadQueue::submit_mesh_upload");
        assert!(!self.destroyed, "MeshUploadQueue used after shutdown");

        let next_value = self.next_timeline_value.checked_add(1).expect("mesh upload timeline exhausted");
        self.pending_uploads.try_reserve(1).expect("mesh upload pending capacity exhausted");
        let stage_count = data.submeshes.len().checked_mul(2).expect("mesh staging count overflow");
        let name = data.name.clone();
        let mut triangle_metadata = Vec::with_capacity(data.submeshes.len());
        let mut geometries = Vec::with_capacity(data.submeshes.len());
        let mut staging_buffers = Vec::with_capacity(stage_count);
        let command_pool = self.command_pool.as_ref().expect("MeshUploadQueue used after shutdown");
        let timeline_semaphore = self.timeline_semaphore.as_ref().expect("MeshUploadQueue used after shutdown");
        let target_value = self.next_timeline_value;
        for (submesh_index, submesh) in data.submeshes.iter().enumerate() {
            // CPU submesh 数据在这里被转换为 render-side SoA 顶点布局与 index buffer。
            // 一个 submesh 对应 BLAS 内一条 geometry，多个 geometry 共享 mesh 级 BLAS 生命周期。
            let vertex_count = submesh.positions.len();
            let index_count = submesh.indices.len();
            let submesh_name =
                if submesh.name.is_empty() { format!("{name}-submesh{submesh_index}") } else { submesh.name.clone() };

            let vertex_buffer = GfxVertexBuffer::<VertexLayoutSoA3D>::new_with_tail(
                resource_ctx,
                vertex_count,
                false,
                VertexLayoutSoA3D::extra_uv_bytes(vertex_count, submesh.tex_coords.len()),
                format!("{submesh_name}-vertex"),
            );
            let index_buffer =
                GfxIndex32Buffer::new_device_local(resource_ctx, index_count, format!("{submesh_name}-index"));
            staging_buffers.push(Self::create_vertex_stage_buffer(
                resource_ctx,
                vertex_count,
                submesh,
                format!("{submesh_name}-vertex-stage"),
            ));
            staging_buffers.push(Self::create_index_stage_buffer(
                resource_ctx,
                &submesh.indices,
                format!("{submesh_name}-index-stage"),
            ));

            geometries.push(RtGeometry {
                vertex_buffer: vertex_buffer,
                index_buffer: index_buffer,
                uv_set_count: submesh.tex_coords.len() as u32,
                tangent_tex_coord: submesh.tangent_tex_coord,
            });
            triangle_metadata.push(Self::build_triangle_metadata(submesh));
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

        // geometry 同时供 BLAS 输入、光栅 vertex/index fetch 和 shader device-address 读取。
        let transfer_to_read_mask = GfxBarrierMask {
            src_stage: vk::PipelineStageFlags2::TRANSFER,
            src_access: vk::AccessFlags2::TRANSFER_WRITE,
            dst_stage: vk::PipelineStageFlags2::ALL_COMMANDS,
            dst_access: vk::AccessFlags2::ACCELERATION_STRUCTURE_READ_KHR
                | vk::AccessFlags2::SHADER_READ
                | vk::AccessFlags2::VERTEX_ATTRIBUTE_READ
                | vk::AccessFlags2::INDEX_READ,
        };
        let mut barriers = Vec::with_capacity(stage_count);
        for geometry in &geometries {
            barriers.push(GfxBufferBarrier::default().mask(transfer_to_read_mask).buffer(
                geometry.vertex_buffer.vk_buffer(),
                0,
                vk::WHOLE_SIZE,
            ));
            barriers.push(GfxBufferBarrier::default().mask(transfer_to_read_mask).buffer(
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
        let submit_info = GfxSubmitInfo::new(std::slice::from_ref(&command_buffer)).signal(
            timeline_semaphore,
            vk::PipelineStageFlags2::ALL_COMMANDS,
            Some(target_value),
        );
        // 提交前完成诊断，native submit 前由 pending 接管资源。
        log::trace!(
            "GpuMeshStore: prepared mesh {:?} '{}' submeshes={} timeline={}",
            handle,
            name,
            geometries.len(),
            target_value
        );
        let batches = vec![submit_info];

        // 从进入 submit 起不能假设提交失败意味着 GPU 未使用；pending 必须先接管完整 owner。
        self.pending_uploads.push_back(SubmittedMeshUpload {
            semaphore_value: target_value,
            handle,
            command_buffer: command_buffer,
            staging_buffers: staging_buffers,
            scratch_buffer: scratch_buffer,
            geometries: geometries,
            triangle_metadata,
            blas: blas,
            name,
        });
        self.next_timeline_value = next_value;

        queue_ctx.gfx_queue().submit(batches, None);
    }

    /// 非阻塞推进上传队列，并将已完成资源交给 consumer。
    fn update(&mut self, resource_ctx: GfxResourceCtx<'_>, device_ctx: GfxDeviceCtx<'_>) -> Vec<FinishedMeshUpload> {
        let _span = tracy_client::span!("MeshUploadQueue::update");

        let device = device_ctx.device();
        let timeline_semaphore = self.timeline_semaphore.as_ref().expect("MeshUploadQueue used after shutdown");
        // 查询失败不能伪装成尚未完成，否则设备错误会使 mesh 永久停在 pending。
        let current_value = unsafe { device.get_semaphore_counter_value(timeline_semaphore.handle()) }
            .expect("failed to query mesh upload timeline");
        self.collect_completed(resource_ctx, device_ctx, current_value, DestroyReason::DeferredCleanup)
    }

    /// 仅消费已确认完成的任务；先保留目标，再回收本次 command/staging/scratch。
    /// 仅释放 timeline 已完成任务的临时资源，将结果交给调用方安装。
    fn collect_completed(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        current_value: u64,
        reason: DestroyReason,
    ) -> Vec<FinishedMeshUpload> {
        let mut completed = Vec::new();
        let command_pool = self.command_pool.as_ref().expect("MeshUploadQueue command pool missing");
        while self.pending_uploads.front().is_some_and(|upload| current_value >= upload.semaphore_value) {
            let upload = self.pending_uploads.pop_front().unwrap();
            command_pool.free_command_buffers(device_ctx, vec![upload.command_buffer]);
            completed.push(FinishedMeshUpload {
                handle: upload.handle,
                geometries: upload.geometries,
                triangle_metadata: upload.triangle_metadata,
                blas: upload.blas,
                name: upload.name,
            });
            for staging_buffer in upload.staging_buffers {
                staging_buffer.destroy(resource_ctx, reason);
            }
            upload.scratch_buffer.destroy(resource_ctx, reason);
        }

        completed
    }

    /// 关闭上传队列并释放仍未交给 GpuMeshStore 的资源。
    /// 超时或设备错误时保留完整 pending 与同步资源并传播失败，不承诺设备恢复。
    fn shutdown(&mut self, resource_ctx: GfxResourceCtx<'_>, device_ctx: GfxDeviceCtx<'_>) {
        if self.destroyed {
            return;
        }
        let timeline_semaphore = self.timeline_semaphore.as_ref().expect("MeshUploadQueue timeline missing");
        let last_value = self.pending_uploads.back().map_or(0, |upload| upload.semaphore_value);
        if last_value != 0 {
            const WAIT_SEMAPHORE_TIMEOUT_NS: u64 = 30 * 1000 * 1000 * 1000;
            timeline_semaphore.wait_timeline(device_ctx, last_value, WAIT_SEMAPHORE_TIMEOUT_NS);
        }
        let completed = self.collect_completed(resource_ctx, device_ctx, last_value, DestroyReason::Shutdown);
        let timeline_semaphore = self.timeline_semaphore.take().expect("MeshUploadQueue timeline missing");
        let mut command_pool = self.command_pool.take().expect("MeshUploadQueue command pool missing");
        for upload in completed {
            UploadedMesh::destroy_resources(
                upload.geometries,
                upload.blas,
                resource_ctx,
                device_ctx,
                DestroyReason::Shutdown,
            );
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
        let total_size = VertexLayoutSoA3D::buffer_size(vertex_count)
            .checked_add(VertexLayoutSoA3D::extra_uv_bytes(vertex_count, data.tex_coords.len()))
            .expect("vertex stage buffer size overflow") as vk::DeviceSize;
        let stage_buffer = GfxBuffer::new_stage_buffer(resource_ctx, total_size, debug_name);
        // CPU registry 已校验属性长度；按 SoA 布局复制 positions/normals/tangents/uvs。
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
            // 固定布局后的 UV 集连续存储；Geometry 用 vertex_count 作为集合 stride。
            for (set, coords) in data.tex_coords.iter().enumerate() {
                ptr::copy_nonoverlapping(
                    coords.as_ptr() as *const u8,
                    stage_buffer.mapped_ptr().add(
                        VertexLayoutSoA3D::uv_offset(vertex_count) as usize
                            + set * vertex_count * size_of::<glam::Vec2>(),
                    ),
                    size_of_val(coords.as_slice()),
                );
            }
            if data.tex_coords.is_empty() {
                ptr::write_bytes(
                    stage_buffer.mapped_ptr().add(VertexLayoutSoA3D::uv_offset(vertex_count) as usize),
                    0,
                    vertex_count * size_of::<glam::Vec2>(),
                );
            }
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
        let mut stage_buffer =
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
    handle: MeshAssetHandle,
    mesh: UploadedMesh,
    retired_frame_id: u64,
}

impl UploadedMesh {
    /// 已完成或已跨过退役安全点的 mesh 资源共用此回收边界。
    fn destroy_resources(
        geometries: Vec<RtGeometry>,
        blas: GfxAcceleration,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        reason: DestroyReason,
    ) {
        for geometry in geometries {
            geometry.destroy(resource_ctx, reason);
        }
        blas.destroy(resource_ctx, device_ctx, reason);
    }

    fn destroy(self, resource_ctx: GfxResourceCtx<'_>, device_ctx: GfxDeviceCtx<'_>, reason: DestroyReason) {
        Self::destroy_resources(self.geometries, self.blas, resource_ctx, device_ctx, reason);
    }
}

/// 渲染侧 mesh 资产上传与 BLAS 缓存。
///
/// 它把 `MeshAssetHandle` 解析为光栅化和 ray tracing 共用的 GPU 几何数据。
pub struct GpuMeshStore {
    meshes: SecondaryMap<MeshAssetHandle, UploadedMesh>,
    pending_meshes: HashSet<MeshAssetHandle>,
    retired_resources: Vec<RetiredMesh>,
    upload_queue: MeshUploadQueue,
    current_frame_id: u64,
}

/// mesh 上传阶段对资源对账暴露的结构化结果。
impl GpuMeshStore {
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
    ) -> Vec<MeshAssetHandle> {
        let _span = tracy_client::span!("GpuMeshStore::sync_scene");
        self.reclaim_retired_resources(resource_ctx, device_ctx);

        for handle in scene.mesh_handles() {
            if !self.needs_upload(handle) {
                continue;
            }
            let Some(data) = scene.mesh_data(handle) else {
                continue;
            };
            self.upload_queue.submit_mesh_upload(resource_ctx, device_ctx, queue_ctx, handle, data);
            self.pending_meshes.insert(handle);
        }

        let mut published_meshes = Vec::new();
        for finished in self.upload_queue.update(resource_ctx, device_ctx) {
            let handle = finished.handle;
            let duplicate = self.meshes.contains_key(handle);
            if !scene.contains_mesh(handle) || duplicate {
                if duplicate {
                    // 日志异常时仍由 completed 队列持有；下一次同步可重新处理。
                    log::error!("GpuMeshStore: reject duplicate upload for immutable mesh {:?}", handle);
                }
                self.pending_meshes.remove(&handle);
                UploadedMesh::destroy_resources(
                    finished.geometries,
                    finished.blas,
                    resource_ctx,
                    device_ctx,
                    DestroyReason::DeferredCleanup,
                );
                continue;
            }
            self.install_uploaded_mesh(device_ctx, finished);
            self.pending_meshes.remove(&handle);
            published_meshes.push(handle);
        }
        published_meshes
    }

    /// 移除 scene mesh 对应的 GPU-ready cache。
    ///
    /// 已提交的上传/BLAS build 只能等待 timeline 自然完成；completion 会再次检查 CPU registry
    /// 中的 generational handle，不会把已删除 mesh 重新发布。
    pub fn remove_meshes(&mut self, handles: &[MeshAssetHandle]) {
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

    pub(crate) fn needs_upload(&self, handle: MeshAssetHandle) -> bool {
        !self.meshes.contains_key(handle) && !self.pending_meshes.contains(&handle)
    }

    /// 按 CPU registry 的完整 membership 清理已经删除的 render-side mesh。
    ///
    /// 首期 mesh 内容不可变；这里仅处理资源生命周期，不支持同 handle 的内容替换。
    pub fn remove_stale_meshes(&mut self, scene: SceneReadView<'_>) -> bool {
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

    fn install_uploaded_mesh(&mut self, device_ctx: GfxDeviceCtx<'_>, finished: FinishedMeshUpload) {
        let blas_device_address = finished.blas.device_address(device_ctx);
        // 缓存 BLAS device address，后续构建 TLAS 时无需重新查询 Vulkan handle。
        log::trace!(
            "GpuMeshStore: mesh {:?} '{}' is GPU ready, blas_address={:#x}",
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
        let mut index = 0;
        while index < self.retired_resources.len() {
            if current_frame_id.saturating_sub(self.retired_resources[index].retired_frame_id) < fif_count {
                index += 1;
                continue;
            }
            // 仅取走已跨过安全窗口的对象。
            let retired = self.retired_resources.swap_remove(index);
            {
                retired.mesh.destroy(resource_ctx, device_ctx, DestroyReason::DeferredCleanup)
            };

            {
                log::debug!("GpuMeshStore: reclaimed mesh {:?} after FIF window", retired.handle);
            };
        }
    }

    /// 关闭上传队列并释放所有 mesh GPU 资源；调用方必须已停止消费者并等待其 GPU 使用完成。
    /// 上传 timeline 只证明上传/build 完成，不能替代已发布 mesh 的 frame 使用安全点。
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

impl MeshRenderResolver for GpuMeshStore {
    fn resolve_mesh(&self, handle: MeshAssetHandle) -> Option<MeshRenderData<'_>> {
        let mesh = self.meshes.get(handle)?;
        Some(MeshRenderData {
            geometries: mesh.geometries.as_slice(),
            triangle_metadata: mesh.triangle_metadata.as_slice(),
            blas_device_address: Some(mesh.blas_device_address),
        })
    }
}
