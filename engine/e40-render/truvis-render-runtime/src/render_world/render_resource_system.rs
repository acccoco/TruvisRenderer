use truvis_gfx::gfx::{GfxDeviceCtx, GfxImmediateCtx, GfxQueueCtx, GfxResourceCtx};
use truvis_render_foundation::frame_label::FrameLabel;
use truvis_world::SceneReadView;

use crate::bindings::shader_binding_system::ShaderBindingSystem;
use crate::render_world::render_asset_upload_queue::RenderAssetUploadQueue;
use crate::render_world::render_material_manager::RenderMaterialManager;
use crate::render_world::render_mesh_manager::RenderMeshManager;
use crate::render_world::render_material_manager::RenderMaterialUpdateResult;
use crate::render_world::render_sky_manager::RenderSkyManager;
use crate::render_world::render_texture_manager::RenderTextureManager;
use crate::resources::gfx_resource_manager::GfxResourceManager;

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
pub(crate) struct RenderResourceSystem {
    pub(crate) asset_upload_queue: RenderAssetUploadQueue,
    pub(crate) texture_manager: RenderTextureManager,
    pub(crate) sky_manager: RenderSkyManager,
    pub(crate) mesh_manager: RenderMeshManager,
    pub(crate) material_manager: RenderMaterialManager,
}

impl RenderResourceSystem {
    pub(crate) fn new(
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        queue_ctx: GfxQueueCtx<'_>,
        gfx_resource_manager: &mut GfxResourceManager,
        shader_binding_system: &mut ShaderBindingSystem,
        current_frame_id: u64,
    ) -> Self {
        Self {
            asset_upload_queue: RenderAssetUploadQueue::new(device_ctx, queue_ctx),
            texture_manager: RenderTextureManager::new(
                resource_ctx,
                device_ctx,
                immediate_ctx,
                gfx_resource_manager,
                shader_binding_system,
            ),
            sky_manager: RenderSkyManager::new(
                resource_ctx,
                device_ctx,
                immediate_ctx,
                gfx_resource_manager,
                shader_binding_system,
                current_frame_id,
            ),
            mesh_manager: RenderMeshManager::new(device_ctx, queue_ctx, current_frame_id),
            material_manager: RenderMaterialManager::new(resource_ctx, current_frame_id),
        }
    }

    pub(crate) fn begin_frame(&mut self, current_frame_id: u64) {
        self.texture_manager.begin_frame(current_frame_id);
        self.mesh_manager.begin_frame(current_frame_id);
        self.sky_manager.begin_frame(current_frame_id);
        self.material_manager.begin_frame(current_frame_id);
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
        gfx_resource_manager: &mut GfxResourceManager,
        shader_binding_system: &mut ShaderBindingSystem,
    ) -> RenderResourceSyncResult {
        let _span = tracy_client::span!("RenderResourceSystem::sync");
        self.texture_manager
            .remove_stale_textures(scene, gfx_resource_manager, shader_binding_system);
        self.mesh_manager.remove_stale_meshes(scene);

        let sky_changed = self
            .sky_manager
            .apply_scene_sky_state(scene.sky_state(), gfx_resource_manager);
        if let Some(texture) = scene.sky_state().texture {
            if let Some(data) = scene.texture_data(texture) {
                self.sky_manager
                    .observe_texture_loaded(texture, data, gfx_resource_manager);
            }
        }

        self.texture_manager.submit_uploads(
            scene,
            resource_ctx,
            device_ctx,
            queue_ctx,
            &mut self.asset_upload_queue,
        );

        self.sky_manager.submit_completed_builds(
            resource_ctx,
            device_ctx,
            queue_ctx,
            &mut self.asset_upload_queue,
        );
        let completed_uploads = self.asset_upload_queue.poll(resource_ctx, device_ctx);
        self.texture_manager.publish_completed_uploads(
            completed_uploads.textures,
            scene,
            resource_ctx,
            device_ctx,
            gfx_resource_manager,
            shader_binding_system,
        );
        self.sky_manager
            .publish_completed_uploads(completed_uploads.sky_distributions, resource_ctx, gfx_resource_manager);

        let material_result: RenderMaterialUpdateResult =
            self.material_manager.sync_scene(scene, &self.texture_manager);
        self.mesh_manager
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
        gfx_resource_manager: &mut GfxResourceManager,
    ) {
        self.material_manager.destroy(resource_ctx);
        self.sky_manager.stop_worker();
        self.asset_upload_queue.shutdown(resource_ctx, device_ctx);
        self.sky_manager.destroy_gpu_resources(
            resource_ctx,
            device_ctx,
            shader_binding_system,
            gfx_resource_manager,
        );
        self.texture_manager
            .destroy(resource_ctx, device_ctx, gfx_resource_manager, shader_binding_system);
        self.mesh_manager.destroy(resource_ctx, device_ctx);
    }

    pub(crate) fn material_buffer_device_address(&self, frame_label: FrameLabel) -> ash::vk::DeviceAddress {
        self.material_manager.material_buffer_device_address(frame_label)
    }

    pub(crate) fn commit_submitted_frame(&mut self, frame_label: FrameLabel) {
        self.material_manager.commit_submitted_frame(frame_label);
    }
}
