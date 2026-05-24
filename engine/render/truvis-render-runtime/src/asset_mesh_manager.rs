use std::collections::VecDeque;
use std::mem::size_of_val;
use std::ptr;

use anyhow::{Result, bail};
use ash::vk;
use slotmap::SecondaryMap;

use crate::render_scene::geometry::RtGeometry;
use crate::render_scene::render_data::MeshRenderData;
use crate::scene_bridge::MeshRenderResolver;
use truvis_asset::asset_hub::AssetLoadedEvent;
use truvis_asset::handle::{AssetMeshHandle, MeshData};
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

/// 已提交到 graphics queue、但尚未确认完成的 mesh 上传任务。
///
/// 这里同时持有 staging/scratch/geometry/BLAS owner，是因为 timeline 到达前这些资源
/// 都仍可能被 copy 或 acceleration build 命令引用，不能交给 resolver 或提前释放。
struct PendingMeshUpload {
    semaphore_value: u64,
    handle: AssetMeshHandle,
    command_buffer: GfxCommandBuffer,
    staging_buffers: Vec<GfxBuffer>,
    scratch_buffer: GfxAccelerationScratchBuffer,
    geometry: RtGeometry,
    blas: GfxAcceleration,
    name: String,
}

/// 已通过 timeline 检测确认完成的 mesh GPU 资源。
///
/// `AssetMeshManager` 接管该结构后，mesh 才会进入 `meshes` map，供 instance bridge
/// 解析为 render-side 几何数据。
struct FinishedMeshUpload {
    handle: AssetMeshHandle,
    geometry: RtGeometry,
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
    pending_uploads: VecDeque<PendingMeshUpload>,
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
        handle: AssetMeshHandle,
        data: MeshData,
    ) -> Result<()> {
        let _span = tracy_client::span!("MeshUploadQueue::submit_mesh_upload");
        Self::validate_mesh_data(&data)?;

        // CPU mesh 数据在这里被转换为 render-side SoA 顶点布局与 index buffer。
        // `RtGeometry` 从创建开始就同时服务光栅化 draw 和 BLAS 输入，避免两套 mesh GPU 表示。
        let vertex_count = data.positions.len();
        let index_count = data.indices.len();
        let name = data.name.clone();

        let vertex_buffer = GfxVertexBuffer::<VertexLayoutSoA3D>::new_device_local(
            resource_ctx,
            vertex_count,
            format!("{name}-vertex"),
        );
        let index_buffer = GfxIndex32Buffer::new_device_local(resource_ctx, index_count, format!("{name}-index"));
        let vertex_stage_buffer =
            Self::create_vertex_stage_buffer(resource_ctx, vertex_count, &data, format!("{name}-vertex-stage"));
        let index_stage_buffer =
            Self::create_index_stage_buffer(resource_ctx, &data.indices, format!("{name}-index-stage"));

        let geometry = RtGeometry {
            vertex_buffer,
            index_buffer,
        };
        // BLAS 输入直接引用刚创建的 device-local vertex/index buffer。后续 command buffer
        // 会先完成 staging copy，再通过 barrier 保证 build 命令读取到复制后的内容。
        let blas_inputs = [geometry.get_blas_geometry_info()];
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
        command_buffer.cmd_copy_buffer(
            &vertex_stage_buffer,
            &geometry.vertex_buffer,
            &[vk::BufferCopy {
                size: vertex_stage_buffer.size(),
                ..Default::default()
            }],
        );
        command_buffer.cmd_copy_buffer(
            &index_stage_buffer,
            &geometry.index_buffer,
            &[vk::BufferCopy {
                size: index_stage_buffer.size(),
                ..Default::default()
            }],
        );

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
        command_buffer.buffer_memory_barrier(
            vk::DependencyFlags::empty(),
            &[
                GfxBufferBarrier::default().mask(transfer_to_blas_mask).buffer(
                    geometry.vertex_buffer.vk_buffer(),
                    0,
                    vk::WHOLE_SIZE,
                ),
                GfxBufferBarrier::default().mask(transfer_to_blas_mask).buffer(
                    geometry.index_buffer.vk_buffer(),
                    0,
                    vk::WHOLE_SIZE,
                ),
            ],
        );

        let geometries = blas_inputs.iter().map(|blas_input| blas_input.geometry).collect::<Vec<_>>();
        let range_infos = blas_inputs.iter().map(|blas_input| blas_input.range).collect::<Vec<_>>();
        let build_geometry_info = vk::AccelerationStructureBuildGeometryInfoKHR::default()
            .ty(vk::AccelerationStructureTypeKHR::BOTTOM_LEVEL)
            .flags(vk::BuildAccelerationStructureFlagsKHR::PREFER_FAST_TRACE)
            .geometries(&geometries)
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

        log::trace!("AssetMeshManager: submitted mesh {:?} '{}' timeline={}", handle, name, target_value);
        self.pending_uploads.push_back(PendingMeshUpload {
            semaphore_value: target_value,
            handle,
            command_buffer,
            staging_buffers: vec![vertex_stage_buffer, index_stage_buffer],
            scratch_buffer,
            geometry,
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
            // geometry 与 BLAS 则转交给 AssetMeshManager，成为 render pass 可解析的数据。
            let upload = self.pending_uploads.pop_front().unwrap();
            command_pool.free_command_buffers(device_ctx, vec![upload.command_buffer]);
            for staging_buffer in upload.staging_buffers {
                staging_buffer.destroy(resource_ctx, DestroyReason::DeferredCleanup);
            }
            upload.scratch_buffer.destroy(resource_ctx, DestroyReason::DeferredCleanup);
            finished_uploads.push(FinishedMeshUpload {
                handle: upload.handle,
                geometry: upload.geometry,
                blas: upload.blas,
                name: upload.name,
            });
        }

        finished_uploads
    }

    /// 关闭上传队列并释放仍未交给 `AssetMeshManager` 的 pending 资源。
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
            upload.geometry.destroy(resource_ctx, DestroyReason::Shutdown);
            upload.blas.destroy(resource_ctx, device_ctx, DestroyReason::Shutdown);
        }

        timeline_semaphore.destroy(device_ctx);
        command_pool.destroy(device_ctx);
        self.destroyed = true;
    }

    /// 在分配 GPU 资源前验证 CPU mesh 数据满足当前渲染运行时的固定假设。
    ///
    /// 当前后端按三角形索引构建 BLAS，并要求 SoA 顶点属性一一对应；这里提前失败，
    /// 避免创建部分 GPU 资源后再在 Vulkan build 阶段暴露难定位的问题。
    fn validate_mesh_data(data: &MeshData) -> Result<()> {
        let vertex_count = data.positions.len();
        if vertex_count == 0 {
            bail!("mesh '{}' has no vertices", data.name);
        }
        if data.normals.len() != vertex_count || data.tangents.len() != vertex_count || data.uvs.len() != vertex_count {
            bail!("mesh '{}' has mismatched vertex attribute counts", data.name);
        }
        if data.indices.is_empty() {
            bail!("mesh '{}' has no indices", data.name);
        }
        if data.indices.len() % 3 != 0 {
            bail!("mesh '{}' index count is not a multiple of 3", data.name);
        }
        Ok(())
    }

    fn create_vertex_stage_buffer(
        resource_ctx: GfxResourceCtx<'_>,
        vertex_count: usize,
        data: &MeshData,
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
}

impl Drop for MeshUploadQueue {
    fn drop(&mut self) {
        debug_assert!(self.destroyed, "MeshUploadQueue dropped without explicit shutdown");
    }
}

/// resolver 可见的 GPU-ready mesh 缓存。
///
/// `geometry` 服务光栅化 draw，`blas`/`blas_device_address` 服务 TLAS 构建；二者共享同一份
/// vertex/index buffer，避免 mesh 在 runtime 内出现两套 GPU 表示。
struct UploadedMesh {
    geometry: RtGeometry,
    blas: GfxAcceleration,
    blas_device_address: vk::DeviceAddress,
}

/// 渲染侧 mesh 资产上传与 BLAS 缓存。
///
/// 它把 `AssetMeshHandle` 解析为光栅化和 ray tracing 共用的 GPU 几何数据。
/// `ready_revision` 在 mesh 首次 ready 或替换时递增，供 `GpuScene` 判断 TLAS 是否需要重建。
pub struct AssetMeshManager {
    meshes: SecondaryMap<AssetMeshHandle, UploadedMesh>,
    upload_queue: MeshUploadQueue,
    ready_revision: u64,
}

impl AssetMeshManager {
    /// 创建 mesh 管理器。
    ///
    /// 内部 command pool 绑定 graphics queue family，因为 BLAS build 不能假设 transfer queue 支持。
    pub fn new(device_ctx: GfxDeviceCtx<'_>, queue_ctx: GfxQueueCtx<'_>) -> Self {
        Self {
            meshes: SecondaryMap::new(),
            upload_queue: MeshUploadQueue::new(device_ctx, queue_ctx),
            ready_revision: 0,
        }
    }

    /// 消费 AssetHub 的 mesh loaded 事件，并推进 GPU 上传/BLAS build 完成检测。
    ///
    /// 该方法只查询 graphics queue timeline semaphore，不等待 GPU；完成前 mesh 不会进入 resolver
    /// 可见的 `meshes` map，因此 instance bridge 会继续把依赖它的实例保持为 pending。
    pub fn update(
        &mut self,
        events: Vec<AssetLoadedEvent>,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        queue_ctx: GfxQueueCtx<'_>,
    ) {
        let _span = tracy_client::span!("AssetMeshManager::update");

        for event in events {
            if let AssetLoadedEvent::MeshLoaded { handle, data } = event {
                // RenderRuntime 已经把事件按类型分流；这里仍保持宽松匹配，便于调用侧
                // 未来传入空列表或过滤后的列表时不需要额外包装。
                if let Err(err) =
                    self.upload_queue.submit_mesh_upload(resource_ctx, device_ctx, queue_ctx, handle, data)
                {
                    log::error!("Failed to submit mesh upload {:?}: {}", handle, err);
                }
            }
        }

        for finished in self.upload_queue.update(resource_ctx, device_ctx) {
            self.replace_uploaded_mesh(resource_ctx, device_ctx, finished);
            self.ready_revision = self.ready_revision.saturating_add(1);
        }
    }

    /// 查询指定 mesh 是否已经完成 vertex/index 上传和 BLAS build。
    pub fn is_mesh_ready(&self, handle: AssetMeshHandle) -> bool {
        self.meshes.contains_key(handle)
    }

    /// 返回 mesh ready 状态的单调递增 revision。
    ///
    /// `RenderRuntime` 会把它与 instance revision 合成 scene revision，供 `GpuScene` 判断
    /// 当前 FIF 的 TLAS 是否需要重建。
    pub fn ready_revision(&self) -> u64 {
        self.ready_revision
    }

    fn replace_uploaded_mesh(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        finished: FinishedMeshUpload,
    ) {
        // 替换同一 asset handle 时必须先让旧资源离开 resolver map；后续 instance bridge
        // 会通过 ready revision 触发 scene/TLAS 更新，而不会继续拿到旧 BLAS。
        if let Some(old_mesh) = self.meshes.remove(finished.handle) {
            // 同一 handle 的 mesh 重新上传时，旧 geometry/BLAS 不能继续被 resolver 返回。
            // 当前实现依赖帧开始的 FIF 等待保证立即释放不会撞上在飞命令。
            old_mesh.geometry.destroy(resource_ctx, DestroyReason::ImmediateRelease);
            old_mesh.blas.destroy(resource_ctx, device_ctx, DestroyReason::ImmediateRelease);
        }

        let blas_device_address = finished.blas.device_address(device_ctx);
        // 缓存 BLAS device address，后续构建 TLAS 时无需重新查询 Vulkan handle。
        log::trace!(
            "AssetMeshManager: mesh {:?} '{}' is GPU ready, blas_address={:#x}",
            finished.handle,
            finished.name,
            blas_device_address
        );
        self.meshes.insert(
            finished.handle,
            UploadedMesh {
                geometry: finished.geometry,
                blas: finished.blas,
                blas_device_address,
            },
        );
    }

    /// 关闭上传队列并释放所有 mesh GPU 资源。
    ///
    /// pending 队列会先等待对应 timeline value，确保 staging/scratch/geometry/BLAS 不再被 graphics queue 引用。
    pub fn destroy(mut self, resource_ctx: GfxResourceCtx<'_>, device_ctx: GfxDeviceCtx<'_>) {
        self.upload_queue.shutdown(resource_ctx, device_ctx);

        for (_, mesh) in self.meshes.drain() {
            mesh.geometry.destroy(resource_ctx, DestroyReason::Shutdown);
            mesh.blas.destroy(resource_ctx, device_ctx, DestroyReason::Shutdown);
        }
    }
}

impl MeshRenderResolver for AssetMeshManager {
    fn is_mesh_ready(&self, handle: AssetMeshHandle) -> bool {
        self.is_mesh_ready(handle)
    }

    fn resolve_mesh(&self, handle: AssetMeshHandle) -> Option<MeshRenderData<'_>> {
        let mesh = self.meshes.get(handle)?;
        Some(MeshRenderData {
            geometries: std::slice::from_ref(&mesh.geometry),
            blas_device_address: Some(mesh.blas_device_address),
        })
    }
}
