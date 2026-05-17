use std::collections::VecDeque;
use std::mem::size_of_val;
use std::ptr;

use anyhow::{Result, bail};
use ash::vk;
use slotmap::SecondaryMap;

use crate::render_scene::geometry::RtGeometry;
use crate::render_scene::render_data::MeshRenderData;
use crate::scene_bridge::MeshRenderResolver;
use truvis_asset::asset_hub::LoadedAssetEvent;
use truvis_asset::handle::{AssetMeshHandle, LoadedMeshData};
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
struct MeshUploadManager {
    command_pool: Option<GfxCommandPool>,
    timeline_semaphore: Option<GfxSemaphore>,
    next_timeline_value: u64,
    pending_uploads: VecDeque<PendingMeshUpload>,
    destroyed: bool,
}

impl MeshUploadManager {
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
        data: LoadedMeshData,
    ) -> Result<()> {
        let _span = tracy_client::span!("MeshUploadManager::submit_mesh_upload");
        Self::validate_mesh_data(&data)?;

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
        let blas_inputs = [geometry.get_blas_geometry_info()];
        let (blas, scratch_buffer) = GfxAcceleration::new_blas_for_build(
            resource_ctx,
            device_ctx,
            &blas_inputs,
            vk::BuildAccelerationStructureFlagsKHR::empty(),
            &name,
        );

        let command_pool = self.command_pool.as_ref().expect("MeshUploadManager used after shutdown");
        let timeline_semaphore = self.timeline_semaphore.as_ref().expect("MeshUploadManager used after shutdown");
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
        let submit_info = GfxSubmitInfo::new(std::slice::from_ref(&command_buffer)).signal(
            timeline_semaphore,
            vk::PipelineStageFlags2::ALL_COMMANDS,
            Some(target_value),
        );
        queue_ctx.gfx_queue().submit(vec![submit_info], None);

        log::trace!("AssetMeshUploader: submitted mesh {:?} '{}' timeline={}", handle, name, target_value);
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

    fn update(&mut self, resource_ctx: GfxResourceCtx<'_>, device_ctx: GfxDeviceCtx<'_>) -> Vec<FinishedMeshUpload> {
        let _span = tracy_client::span!("MeshUploadManager::update");
        let device = device_ctx.device();
        let timeline_semaphore = self.timeline_semaphore.as_ref().expect("MeshUploadManager used after shutdown");
        let command_pool = self.command_pool.as_ref().expect("MeshUploadManager used after shutdown");
        let current_value = unsafe { device.get_semaphore_counter_value(timeline_semaphore.handle()).unwrap_or(0) };

        let mut finished_uploads = Vec::new();
        while let Some(upload) = self.pending_uploads.front() {
            if current_value < upload.semaphore_value {
                break;
            }

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

    fn shutdown(&mut self, resource_ctx: GfxResourceCtx<'_>, device_ctx: GfxDeviceCtx<'_>) {
        if self.destroyed {
            return;
        }

        let Some(timeline_semaphore) = self.timeline_semaphore.take() else {
            self.destroyed = true;
            return;
        };
        let mut command_pool = self.command_pool.take().expect("MeshUploadManager command pool missing");

        if let Some(last_upload) = self.pending_uploads.back() {
            const WAIT_SEMAPHORE_TIMEOUT_NS: u64 = 30 * 1000 * 1000 * 1000;
            timeline_semaphore.wait_timeline(device_ctx, last_upload.semaphore_value, WAIT_SEMAPHORE_TIMEOUT_NS);
        }

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

    fn validate_mesh_data(data: &LoadedMeshData) -> Result<()> {
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
        data: &LoadedMeshData,
        debug_name: impl AsRef<str>,
    ) -> GfxBuffer {
        let total_size = VertexLayoutSoA3D::buffer_size(vertex_count) as vk::DeviceSize;
        let stage_buffer = GfxBuffer::new_stage_buffer(resource_ctx, total_size, debug_name);
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
        let stage_buffer =
            GfxBuffer::new_stage_buffer(resource_ctx, size_of_val(indices) as vk::DeviceSize, debug_name);
        stage_buffer.transfer_data_by_mmap(resource_ctx, indices);
        stage_buffer
    }
}

impl Drop for MeshUploadManager {
    fn drop(&mut self) {
        debug_assert!(self.destroyed, "MeshUploadManager dropped without explicit shutdown");
    }
}

struct UploadedMesh {
    geometry: RtGeometry,
    blas: GfxAcceleration,
    blas_device_address: vk::DeviceAddress,
}

/// 渲染侧 mesh 资产上传与 BLAS 缓存。
pub struct AssetMeshUploader {
    meshes: SecondaryMap<AssetMeshHandle, UploadedMesh>,
    uploader: MeshUploadManager,
    ready_revision: u64,
}

impl AssetMeshUploader {
    pub fn new(device_ctx: GfxDeviceCtx<'_>, queue_ctx: GfxQueueCtx<'_>) -> Self {
        Self {
            meshes: SecondaryMap::new(),
            uploader: MeshUploadManager::new(device_ctx, queue_ctx),
            ready_revision: 0,
        }
    }

    pub fn update(
        &mut self,
        events: Vec<LoadedAssetEvent>,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        queue_ctx: GfxQueueCtx<'_>,
    ) {
        let _span = tracy_client::span!("AssetMeshUploader::update");

        for event in events {
            if let LoadedAssetEvent::MeshLoaded { handle, data } = event {
                if let Err(err) = self.uploader.submit_mesh_upload(resource_ctx, device_ctx, queue_ctx, handle, data) {
                    log::error!("Failed to submit mesh upload {:?}: {}", handle, err);
                }
            }
        }

        for finished in self.uploader.update(resource_ctx, device_ctx) {
            self.replace_uploaded_mesh(resource_ctx, device_ctx, finished);
            self.ready_revision = self.ready_revision.saturating_add(1);
        }
    }

    pub fn is_mesh_ready(&self, handle: AssetMeshHandle) -> bool {
        self.meshes.contains_key(handle)
    }

    pub fn ready_revision(&self) -> u64 {
        self.ready_revision
    }

    fn replace_uploaded_mesh(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        finished: FinishedMeshUpload,
    ) {
        if let Some(old_mesh) = self.meshes.remove(finished.handle) {
            old_mesh.geometry.destroy(resource_ctx, DestroyReason::ImmediateRelease);
            old_mesh.blas.destroy(resource_ctx, device_ctx, DestroyReason::ImmediateRelease);
        }

        let blas_device_address = finished.blas.device_address(device_ctx);
        log::trace!(
            "AssetMeshUploader: mesh {:?} '{}' is GPU ready, blas_address={:#x}",
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

    pub fn destroy(mut self, resource_ctx: GfxResourceCtx<'_>, device_ctx: GfxDeviceCtx<'_>) {
        self.uploader.shutdown(resource_ctx, device_ctx);

        for (_, mesh) in self.meshes.drain() {
            mesh.geometry.destroy(resource_ctx, DestroyReason::Shutdown);
            mesh.blas.destroy(resource_ctx, device_ctx, DestroyReason::Shutdown);
        }
    }
}

impl MeshRenderResolver for AssetMeshUploader {
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
