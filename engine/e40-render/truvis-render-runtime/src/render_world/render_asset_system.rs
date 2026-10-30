use truvis_gfx::gfx::{GfxDeviceCtx, GfxImmediateCtx, GfxQueueCtx, GfxResourceCtx};
use truvis_render_foundation::frame_label::FrameLabel;
use truvis_world::SceneReadView;

use crate::bindings::shader_binding_system::ShaderBindingSystem;
use crate::render_world::gpu_asset_upload_queue::GpuAssetUploadQueue;
use crate::render_world::gpu_material_store::GpuMaterialStore;
use crate::render_world::gpu_material_store::RenderMaterialUpdateResult;
use crate::render_world::gpu_mesh_store::GpuMeshStore;
use crate::render_world::gpu_sky_store::{GpuSkyStore, RenderSkyUpdateResult};
use crate::render_world::gpu_texture_store::GpuTextureStore;
use crate::resources::gfx_resource_registry::GfxResourceRegistry;

/// 尚待 RenderWorld 消费的资源变化；每个叶子 owner 成功返回后即移交，后续失败不能丢弃。
///
/// 资源 owner 自己消费上传完成事件；mesh 发布与材质投影变化经 CPU 反向引用触发
/// instance 局部对账。实际 ready 仍由 resolver 判断，不能将上传入队当作发布。
#[derive(Default)]
pub(crate) struct RenderResourceSyncResult {
    pub(crate) published_meshes: indexmap::IndexSet<truvis_world::guid_new_type::MeshAssetHandle>,
    pub(crate) appearance_changed_materials: indexmap::IndexSet<truvis_world::guid_new_type::MaterialAssetHandle>,
    pub(crate) emissive_changed_materials: indexmap::IndexSet<truvis_world::guid_new_type::MaterialAssetHandle>,
    pub(crate) sky_changed: bool,
}

/// 当前 `RenderWorld` 内的共享 GPU 资源 owner。
///
/// 资源按 CPU handle 管理，供同一个 RenderWorld 内的多个 instance 共享；它不属于某个
/// instance，也不把资源同步逻辑复制到 scene mirror 中。未来若需要跨 RenderWorld 共享，
/// 应单独抽取 device-level owner，而不是让本类型隐式跨 world 共享。
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
        let upload_queue = GpuAssetUploadQueue::new(device_ctx, queue_ctx);
        let gpu_textures;
        let gpu_sky;
        let gpu_meshes;
        let initialized = {
            gpu_textures = Some(GpuTextureStore::new(
                resource_ctx,
                device_ctx,
                immediate_ctx,
                gfx_resource_registry,
                shader_binding_system,
            ));
            gpu_sky = Some(GpuSkyStore::new(
                resource_ctx,
                device_ctx,
                immediate_ctx,
                gfx_resource_registry,
                shader_binding_system,
                current_frame_id,
            ));
            gpu_meshes = Some(GpuMeshStore::new(device_ctx, queue_ctx, current_frame_id));
            GpuMaterialStore::new(resource_ctx, current_frame_id)
        };
        let gpu_materials = initialized;
        Self {
            upload_queue,
            gpu_textures: gpu_textures.unwrap(),
            gpu_sky: gpu_sky.unwrap(),
            gpu_meshes: gpu_meshes.unwrap(),
            gpu_materials,
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
        changes: &mut RenderResourceSyncResult,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        queue_ctx: GfxQueueCtx<'_>,
        gfx_resource_registry: &mut GfxResourceRegistry,
        shader_binding_system: &mut ShaderBindingSystem,
    ) -> Option<(ash::vk::Semaphore, u64)> {
        let _span = tracy_client::span!("RenderAssetSystem::sync");
        self.gpu_textures
            .remove_stale_textures(scene, gfx_resource_registry, shader_binding_system);
        self.gpu_meshes.remove_stale_meshes(scene);

        changes.sky_changed |= self
            .gpu_sky
            .apply_scene_sky_state(scene.sky_state(), gfx_resource_registry);
        if let Some(texture) = scene.sky_state().texture {
            if let Some(data) = scene.texture_data(texture) {
                self.gpu_sky
                    .observe_texture_loaded(texture, data, gfx_resource_registry);
            }
        }

        self.gpu_textures.submit_uploads(scene, resource_ctx, device_ctx, queue_ctx, &mut self.upload_queue);

        self.gpu_sky.submit_completed_builds(resource_ctx, device_ctx, queue_ctx, &mut self.upload_queue);
        let mut completed_uploads = self.upload_queue.poll(resource_ctx, device_ctx);
        self.gpu_textures.publish_completed_uploads(
            &mut completed_uploads.textures,
            scene,
            resource_ctx,
            device_ctx,
            gfx_resource_registry,
            shader_binding_system,
        );
        self.gpu_sky.publish_completed_uploads(
            &mut completed_uploads.sky_distributions,
            resource_ctx,
            gfx_resource_registry,
        );

        let material_result: RenderMaterialUpdateResult =
            self.gpu_materials.sync_scene(scene, &self.gpu_textures);
        changes.appearance_changed_materials.extend(material_result.appearance_changed_materials);
        changes.emissive_changed_materials.extend(material_result.emissive_changed_materials);
        changes.published_meshes.extend(self.gpu_meshes.sync_scene(scene, resource_ctx, device_ctx, queue_ctx));

        // wait 借用仍属于 queue 的 semaphore，通知集合不延长裸句柄的使用范围。
        self.upload_queue.graphics_wait()
    }

    /// 调用前 Runtime 已停止消费者并等待已发布资源的 graphics 使用结束。
    /// 先停止 producer，再关闭共享 queue；仅确认 queue 已关闭后才继续各 owner 的回收。
    pub(crate) fn destroy(
        mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        shader_binding_system: &mut ShaderBindingSystem,
        gfx_resource_registry: &mut GfxResourceRegistry,
    ) {
        // 先停止 CPU producer，避免关闭上传 queue 时产生新任务。
        self.gpu_sky.stop_worker();
        self.upload_queue.shutdown(resource_ctx, device_ctx);

        self.gpu_materials.destroy(resource_ctx);

        self.gpu_sky.destroy_gpu_resources(resource_ctx, device_ctx, shader_binding_system, gfx_resource_registry);

        self.gpu_textures.destroy(resource_ctx, device_ctx, gfx_resource_registry, shader_binding_system);

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
