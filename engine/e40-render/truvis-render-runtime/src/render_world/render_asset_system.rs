use truvis_gfx::gfx::{GfxDeviceCtx, GfxImmediateCtx, GfxQueueCtx, GfxResourceCtx};
use truvis_render_foundation::frame_label::FrameLabel;
use truvis_world::SceneReadView;

use crate::bindings::shader_binding_system::ShaderBindingSystem;
use crate::render_world::gpu_asset_upload_queue::GpuAssetUploadQueue;
use crate::render_world::gpu_material_store::GpuMaterialStore;
use crate::render_world::gpu_mesh_store::GpuMeshStore;
use crate::render_world::gpu_material_store::RenderMaterialUpdateResult;
use crate::render_world::gpu_sky_store::{GpuSkyStore, RenderSkyUpdateResult};
use crate::render_world::gpu_texture_store::GpuTextureStore;
use crate::resources::gfx_resource_registry::GfxResourceRegistry;

/// 一次资源对账对 RenderWorld 的最小结果。
///
/// 资源 owner 自己消费上传完成事件；RenderWorld 只需要知道哪些材质投影和 sky 绑定
/// 可能影响场景派生历史。texture/mesh 是否 ready 由 resolver 在 instance 阶段直接判断。
pub(crate) struct RenderResourceSyncResult {
    pub(crate) appearance_changed_materials: Vec<truvis_world::guid_new_type::MaterialHandle>,
    pub(crate) emissive_changed_materials: Vec<truvis_world::guid_new_type::MaterialHandle>,
    pub(crate) sky_changed: bool,
}

/// 当前 device 的共享 GPU 资源 owner。
///
/// 资源按 CPU handle 管理，和具体 `RenderWorld` 的 instance / TLAS 组合无关。一个 runtime
/// 当前只有一个 RenderWorld，但把这个边界单独保留下来后，资源同步不会再随着场景镜像扩散。
pub(crate) struct RenderAssetSystem {
    upload_queue: GpuAssetUploadQueue,
    gpu_textures: GpuTextureStore,
    gpu_sky: GpuSkyStore,
    gpu_meshes: GpuMeshStore,
    gpu_materials: GpuMaterialStore,
}

impl RenderAssetSystem {
    pub(crate) fn new(
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        queue_ctx: GfxQueueCtx<'_>,
        gfx_resource_registry: &mut GfxResourceRegistry,
        shader_binding_system: &mut ShaderBindingSystem,
        current_frame_id: u64,
    ) -> Self {
        Self {
            upload_queue: GpuAssetUploadQueue::new(device_ctx, queue_ctx),
            gpu_textures: GpuTextureStore::new(
                resource_ctx,
                device_ctx,
                immediate_ctx,
                gfx_resource_registry,
                shader_binding_system,
            ),
            gpu_sky: GpuSkyStore::new(
                resource_ctx,
                device_ctx,
                immediate_ctx,
                gfx_resource_registry,
                shader_binding_system,
                current_frame_id,
            ),
            gpu_meshes: GpuMeshStore::new(device_ctx, queue_ctx, current_frame_id),
            gpu_materials: GpuMaterialStore::new(resource_ctx, current_frame_id),
        }
    }

    pub(crate) fn begin_frame(&mut self, current_frame_id: u64) {
        self.gpu_textures.begin_frame(current_frame_id);
        self.gpu_meshes.begin_frame(current_frame_id);
        self.gpu_sky.begin_frame(current_frame_id);
        self.gpu_materials.begin_frame(current_frame_id);
    }

    /// 对账 CPU 资源表并推进本帧的 GPU 资源安装。
    ///
    /// CPU registry 是最终 membership 与内容真相；上传完成后才把资源发布给 resolver。
    pub(crate) fn sync(
        &mut self,
        scene: SceneReadView<'_>,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        queue_ctx: GfxQueueCtx<'_>,
        gfx_resource_registry: &mut GfxResourceRegistry,
        shader_binding_system: &mut ShaderBindingSystem,
    ) -> RenderResourceSyncResult {
        let _span = tracy_client::span!("RenderAssetSystem::sync");
        self.gpu_textures
            .remove_stale_textures(scene, gfx_resource_registry, shader_binding_system);
        self.gpu_meshes.remove_stale_meshes(scene);

        let sky_changed = self
            .gpu_sky
            .apply_scene_sky_state(scene.sky_state(), gfx_resource_registry);
        if let Some(texture) = scene.sky_state().texture {
            if let Some(data) = scene.texture_data(texture) {
                self.gpu_sky
                    .observe_texture_loaded(texture, data, gfx_resource_registry);
            }
        }

        self.gpu_textures.submit_uploads(
            scene,
            resource_ctx,
            device_ctx,
            queue_ctx,
            &mut self.upload_queue,
        );

        self.gpu_sky.submit_completed_builds(
            resource_ctx,
            device_ctx,
            queue_ctx,
            &mut self.upload_queue,
        );
        let completed_uploads = self.upload_queue.poll(resource_ctx, device_ctx);
        self.gpu_textures.publish_completed_uploads(
            completed_uploads.textures,
            scene,
            resource_ctx,
            device_ctx,
            gfx_resource_registry,
            shader_binding_system,
        );
        self.gpu_sky
            .publish_completed_uploads(completed_uploads.sky_distributions, resource_ctx, gfx_resource_registry);

        let material_result: RenderMaterialUpdateResult =
            self.gpu_materials.sync_scene(scene, &self.gpu_textures);
        self.gpu_meshes
            .sync_scene(scene, resource_ctx, device_ctx, queue_ctx);

        RenderResourceSyncResult {
            appearance_changed_materials: material_result.appearance_changed_materials,
            emissive_changed_materials: material_result.emissive_changed_materials,
            sky_changed,
        }
    }

    pub(crate) fn destroy(
        mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        shader_binding_system: &mut ShaderBindingSystem,
        gfx_resource_registry: &mut GfxResourceRegistry,
    ) {
        self.gpu_materials.destroy(resource_ctx);
        self.gpu_sky.stop_worker();
        self.upload_queue.shutdown(resource_ctx, device_ctx);
        self.gpu_sky.destroy_gpu_resources(
            resource_ctx,
            device_ctx,
            shader_binding_system,
            gfx_resource_registry,
        );
        self.gpu_textures
            .destroy(resource_ctx, device_ctx, gfx_resource_registry, shader_binding_system);
        self.gpu_meshes.destroy(resource_ctx, device_ctx);
    }

    pub(crate) fn material_buffer_device_address(&self, frame_label: FrameLabel) -> ash::vk::DeviceAddress {
        self.gpu_materials.material_buffer_device_address(frame_label)
    }

    pub(crate) fn update_sky_binding(&mut self) -> RenderSkyUpdateResult {
        self.gpu_sky.update_sky_binding(&self.gpu_textures)
    }

    pub(crate) fn upload_materials(
        &mut self,
        resource_ctx: truvis_gfx::gfx::GfxResourceCtx<'_>,
        cmd: &truvis_gfx::commands::command_buffer::GfxCommandBuffer,
        transfer_barrier_mask: truvis_gfx::commands::barrier::GfxBarrierMask,
        frame_label: FrameLabel,
    ) {
        self.gpu_materials.upload(resource_ctx, cmd, transfer_barrier_mask, frame_label);
    }

    pub(crate) fn material_resolver(&self) -> &GpuMaterialStore {
        &self.gpu_materials
    }

    pub(crate) fn mesh_resolver(&self) -> &GpuMeshStore {
        &self.gpu_meshes
    }

    pub(crate) fn commit_submitted_frame(&mut self, frame_label: FrameLabel) {
        self.gpu_materials.commit_submitted_frame(frame_label);
    }
}
