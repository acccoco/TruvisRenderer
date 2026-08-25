use ash::vk;

use truvis_gfx::basic::bytes::BytesConvert;
use truvis_gfx::commands::barrier::{GfxBarrierMask, GfxBufferBarrier};
use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_gfx::gfx::{GfxDeviceCtx, GfxImmediateCtx, GfxQueueCtx, GfxResourceCtx};
use truvis_gfx::raytracing::acceleration::GfxAcceleration;
use truvis_gfx::resources::buffer::GfxBuffer;
use truvis_gfx::resources::special_buffers::structured_buffer::GfxStructuredBuffer;
use truvis_render_foundation::frame_counter::{FrameLabel, FrameToken};
use truvis_render_foundation::render_scene_view::{RenderSceneAccumSignature, RenderSceneView};
use truvis_shader_binding::gpu;
use truvis_world::SceneReadView;
use truvis_world::{SceneAssetSyncOutput, SceneChanges};

use crate::bindings::shader_binding_system::ShaderBindingSystem;
use crate::render_world::dirty_router::{DirtyDispatchPlan, DirtyRouterHelper, DirtyStageKind};
use crate::render_world::environment_binding::EnvironmentBinding;
use crate::render_world::render_analytic_light_manager::{AnalyticLightBinding, RenderAnalyticLightManager};
use crate::render_world::render_asset_upload_queue::RenderAssetUploadQueue;
use crate::render_world::render_data::RenderData;
use crate::render_world::render_emissive_light_table::EmissiveLightBinding;
use crate::render_world::render_emissive_light_table::RenderEmissiveLightTable;
use crate::render_world::render_instance_manager::RenderInstanceManager;
use crate::render_world::render_material_manager::RenderMaterialManager;
use crate::render_world::render_mesh_manager::RenderMeshManager;
use crate::render_world::render_sky_manager::RenderSkyManager;
use crate::render_world::render_texture_manager::RenderTextureManager;
use crate::render_world::render_tlas_manager::RenderTlasManager;
use crate::resources::gfx_resource_manager::GfxResourceManager;
use crate::selection::{WorldSubmeshRasterView, WorldSubmeshSelection};

use super::buffers::RenderWorldBuffers;
use super::raster_draw_cache::{
    RasterDrawItem, draw_raster_cache, draw_raster_cache_selected, update_raster_draw_cache,
};

/// runtime 私有的 GPU scene 翻译层。
///
/// 它把 `RenderInstanceManager` 产出的 `RenderData` 转换成 shader 可读的 GPU buffer 和
/// 光栅化 draw cache，并协调 `RenderTlasManager` 更新 TLAS；render pass 只能通过
/// `RenderSceneView` 读取 prepare 后的快照。
/// `RenderWorld` 不拥有 CPU scene；它拥有 render-side managers 和当前 FIF 可用的 GPU scene 表示。
pub struct RenderWorld {
    /// texture image 与 sky distribution 共用的异步 transfer owner。
    ///
    /// queue 在任何 manager 发布资源前以 timeline 完成作为唯一 ready 条件，并在 shutdown
    /// 时晚于 CPU producer 停止、早于 manager-owned GPU 资源销毁。
    asset_upload_queue: RenderAssetUploadQueue,
    /// render-side scene managers 统一收敛在 `RenderWorld` 内部，避免 `RenderRuntime` 直接编排各类
    /// texture/mesh/material/instance/sky/emissive 资源 owner。
    pub(super) render_texture_manager: RenderTextureManager,
    pub(super) render_sky_manager: RenderSkyManager,
    pub(super) render_mesh_manager: RenderMeshManager,
    pub(super) render_material_manager: RenderMaterialManager,
    pub(super) render_instance_manager: RenderInstanceManager,
    pub(super) render_analytic_light_manager: RenderAnalyticLightManager,
    pub(super) render_emissive_light_table: RenderEmissiveLightTable,
    /// 每个 FIF frame label 独占一套 scene buffer，避免覆盖 GPU 仍在读取的数据。
    pub(super) render_world_buffers: [RenderWorldBuffers; FrameLabel::COUNT],
    /// TLAS 由独立 manager 持有，避免 acceleration structure 生命周期散在 scene buffer 里。
    pub(super) render_tlas_manager: RenderTlasManager,
    /// prepare 阶段从 `RenderData` 展开的光栅化 draw cache，render pass 只通过 view 契约录制 draw。
    pub(super) raster_draws: [Vec<RasterDrawItem>; FrameLabel::COUNT],
}

pub(crate) struct RenderWorldPrepareResult {
    pub(crate) sky_changed: bool,
}

pub(crate) struct RenderWorldAssetSyncResult {
    pub(crate) dirty_dispatch_plan: DirtyDispatchPlan,
}

// 生命周期：创建和销毁 `RenderWorld` 拥有的长期 GPU 资源。
impl RenderWorld {
    /// 创建 render-side world 拥有的长期 GPU scene 资源和各 render manager。
    ///
    /// `RenderWorld` 不拥有 CPU scene；它只接收 asset sync、scene read view 和当前 frame
    /// label，把这些输入整理成 GPU 可见的 scene 表示。
    pub fn new(
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        queue_ctx: GfxQueueCtx<'_>,
        gfx_resource_manager: &mut GfxResourceManager,
        shader_binding_system: &mut ShaderBindingSystem,
        frame_token: FrameToken,
    ) -> Self {
        let _span = tracy_client::span!("RenderWorld::new");
        let asset_upload_queue = {
            let _span = tracy_client::span!("RenderWorld::new/asset_upload_queue");
            RenderAssetUploadQueue::new(device_ctx, queue_ctx)
        };
        let render_texture_manager = {
            let _span = tracy_client::span!("RenderWorld::new/render_texture_manager");
            RenderTextureManager::new(
                resource_ctx,
                device_ctx,
                immediate_ctx,
                gfx_resource_manager,
                shader_binding_system,
            )
        };
        let render_mesh_manager = {
            let _span = tracy_client::span!("RenderWorld::new/render_mesh_manager");
            RenderMeshManager::new(device_ctx, queue_ctx)
        };
        let render_material_manager = {
            let _span = tracy_client::span!("RenderWorld::new/render_material_manager");
            RenderMaterialManager::new(resource_ctx, frame_token)
        };
        let render_instance_manager = {
            let _span = tracy_client::span!("RenderWorld::new/render_instance_manager");
            RenderInstanceManager::new(frame_token)
        };
        let render_sky_manager = {
            let _span = tracy_client::span!("RenderWorld::new/render_sky_manager");
            RenderSkyManager::new(
                resource_ctx,
                device_ctx,
                immediate_ctx,
                gfx_resource_manager,
                shader_binding_system,
                frame_token,
            )
        };
        let render_emissive_light_table = {
            let _span = tracy_client::span!("RenderWorld::new/render_emissive_light_table");
            RenderEmissiveLightTable::new(resource_ctx)
        };
        let render_analytic_light_manager = {
            let _span = tracy_client::span!("RenderWorld::new/render_analytic_light_manager");
            RenderAnalyticLightManager::new(resource_ctx)
        };

        let render_world_buffers = {
            let _span = tracy_client::span!("RenderWorld::new/per_frame_buffers");
            FrameLabel::ALL.map(|frame_label| RenderWorldBuffers::new(resource_ctx, frame_label))
        };

        Self {
            asset_upload_queue,
            render_texture_manager,
            render_sky_manager,
            render_mesh_manager,
            render_material_manager,
            render_instance_manager,
            render_analytic_light_manager,
            render_emissive_light_table,
            render_world_buffers,
            render_tlas_manager: RenderTlasManager::new(),
            raster_draws: FrameLabel::ALL.map(|_| Vec::new()),
        }
    }

    /// 销毁 render-side world 拥有的全部 GPU scene 资源。
    ///
    /// 调用点位于 `RenderRuntime::destroy`，此时 device 已 idle，因此 manager 资源、每个 FIF 的
    /// TLAS 和 buffer 都可以按 shutdown reason 释放。
    pub fn destroy(
        mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        shader_binding_system: &mut ShaderBindingSystem,
        gfx_resource_manager: &mut GfxResourceManager,
    ) {
        self.render_material_manager.destroy(resource_ctx);
        // shutdown 顺序是协议的一部分：先停止 CPU producer，再等待/释放 pending
        // transfer，最后销毁已经发布的 manager resources。
        self.render_sky_manager.stop_worker();
        self.asset_upload_queue.shutdown(resource_ctx, device_ctx);
        self.render_sky_manager.destroy_gpu_resources(
            resource_ctx,
            device_ctx,
            shader_binding_system,
            gfx_resource_manager,
        );
        self.render_texture_manager.destroy(resource_ctx, device_ctx, gfx_resource_manager, shader_binding_system);
        self.render_analytic_light_manager.destroy_mut(resource_ctx);
        self.render_emissive_light_table.destroy_mut(resource_ctx);
        self.render_tlas_manager.destroy_mut(resource_ctx, device_ctx);
        for buffers in &mut self.render_world_buffers {
            buffers.destroy_mut(resource_ctx, device_ctx);
        }
        self.render_mesh_manager.destroy(resource_ctx, device_ctx);
    }
}

// Runtime 内部阶段入口：`RenderRuntime` 只负责提供阶段上下文，具体 render-side scene 状态在这里推进。
impl RenderWorld {
    pub(crate) fn begin_frame(&mut self, frame_token: FrameToken) {
        self.render_sky_manager.begin_frame(frame_token);
        self.render_material_manager.begin_frame(frame_token);
        self.render_instance_manager.begin_frame(frame_token);
    }

    pub(crate) fn request_motion_history_reset(&mut self) {
        self.render_instance_manager.request_motion_history_reset();
    }

    /// 同步 raycast 需要用 instance slot 反查 CPU record；只暴露只读引用，不暴露 instance manager 修改入口。
    pub(crate) fn render_instance_manager(&self) -> &RenderInstanceManager {
        &self.render_instance_manager
    }

    /// 消费 `World::sync_for_render` 产出的 asset sync payload，并转发给对应 render-side owner。
    ///
    /// texture 与 mesh 事件会进入 GPU 上传队列；material 事件会进入稳定 slot 映射。
    /// model ready/failed 状态由 app 通过 `World` facade 查询，不通过 render-side sync payload 暴露。
    pub(crate) fn prepare_asset_sync(
        &mut self,
        asset_uploads: SceneAssetSyncOutput,
        scene_changes: &SceneChanges,
        scene: SceneReadView<'_>,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        _immediate_ctx: GfxImmediateCtx<'_>,
        queue_ctx: GfxQueueCtx<'_>,
        gfx_resource_manager: &mut GfxResourceManager,
        shader_binding_system: &mut ShaderBindingSystem,
    ) -> RenderWorldAssetSyncResult {
        let _span = tracy_client::span!("RenderWorld::prepare_asset_sync");
        let scene_events = DirtyRouterHelper::events_from_scene_changes(scene_changes);
        let mut dirty_dispatch_plan = DirtyDispatchPlan::default();
        for stage in [
            DirtyStageKind::Texture,
            DirtyStageKind::Sky,
            DirtyStageKind::Material,
            DirtyStageKind::Mesh,
            DirtyStageKind::Instance,
            DirtyStageKind::Analytic,
        ] {
            DirtyRouterHelper::route_stage(stage, &scene_events, scene, &mut dirty_dispatch_plan);
        }

        let texture_removals = dirty_dispatch_plan.take_texture_removals();
        self.render_texture_manager.remove_textures(
            &texture_removals,
            resource_ctx,
            device_ctx,
            gfx_resource_manager,
            shader_binding_system,
        );
        let mesh_removals = dirty_dispatch_plan.take_mesh_removals();
        self.render_mesh_manager.remove_meshes(&mesh_removals, resource_ctx, device_ctx);

        if dirty_dispatch_plan.take_sky_dirty() {
            self.render_sky_manager.apply_scene_sky_state(scene.sky_state(), gfx_resource_manager);
        }

        let material_result =
            self.render_material_manager.apply_material_dispatch(scene, dirty_dispatch_plan.take_material_dispatch());
        let material_events = DirtyRouterHelper::events_from_material_update_result(material_result);
        DirtyRouterHelper::route_stage(
            DirtyStageKind::AfterMaterial,
            &material_events,
            scene,
            &mut dirty_dispatch_plan,
        );

        for upload in &asset_uploads.pending_texture_uploads {
            self.render_sky_manager.observe_texture_loaded(upload.handle, &upload.data, gfx_resource_manager);
        }
        for failed in &asset_uploads.failed_textures {
            self.render_sky_manager.observe_texture_failed(failed.handle, &failed.error, gfx_resource_manager);
        }

        self.render_texture_manager.submit_uploads(
            asset_uploads.pending_texture_uploads,
            asset_uploads.failed_textures,
            resource_ctx,
            device_ctx,
            queue_ctx,
            &mut self.asset_upload_queue,
        );
        self.render_sky_manager.submit_completed_builds(
            resource_ctx,
            device_ctx,
            queue_ctx,
            &mut self.asset_upload_queue,
        );
        let completed_uploads = self.asset_upload_queue.poll(resource_ctx, device_ctx);
        let texture_result = self.render_texture_manager.publish_completed_uploads(
            completed_uploads.textures,
            resource_ctx,
            device_ctx,
            gfx_resource_manager,
            shader_binding_system,
        );
        self.render_sky_manager.publish_completed_uploads(
            completed_uploads.sky_distributions,
            resource_ctx,
            gfx_resource_manager,
        );
        let texture_events = DirtyRouterHelper::events_from_texture_update_result(texture_result);
        DirtyRouterHelper::route_stage(DirtyStageKind::AfterTexture, &texture_events, scene, &mut dirty_dispatch_plan);
        let material_result =
            self.render_material_manager.apply_material_dispatch(scene, dirty_dispatch_plan.take_material_dispatch());
        let material_events = DirtyRouterHelper::events_from_material_update_result(material_result);
        DirtyRouterHelper::route_stage(
            DirtyStageKind::AfterMaterial,
            &material_events,
            scene,
            &mut dirty_dispatch_plan,
        );

        let mesh_result =
            self.render_mesh_manager.update(asset_uploads.pending_mesh_uploads, resource_ctx, device_ctx, queue_ctx);
        let mesh_events = DirtyRouterHelper::events_from_mesh_update_result(mesh_result);
        DirtyRouterHelper::route_stage(DirtyStageKind::AfterMesh, &mesh_events, scene, &mut dirty_dispatch_plan);

        RenderWorldAssetSyncResult { dirty_dispatch_plan }
    }

    /// 准备 render pass 可见的 GPU scene。
    ///
    /// 该函数保持现有 prepare 顺序：sky binding、material buffer、instance ready gate、
    /// emissive table、TLAS/scene root。它不修改 CPU scene，只读取 `SceneReadView` 快照。
    pub(crate) fn prepare_render_data(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        cmd: &GfxCommandBuffer,
        transfer_barrier_mask: GfxBarrierMask,
        frame_id: u64,
        frame_label: FrameLabel,
        scene: SceneReadView<'_>,
        dirty_dispatch_plan: DirtyDispatchPlan,
    ) -> RenderWorldPrepareResult {
        let mut dirty_dispatch_plan = dirty_dispatch_plan;
        let _sky_dirty = dirty_dispatch_plan.take_sky_dirty();
        let sky_update = self.render_sky_manager.update_sky_binding(&self.render_texture_manager);
        let environment_binding = EnvironmentBinding {
            sky: sky_update.binding,
        };

        let material_result =
            self.render_material_manager.apply_material_dispatch(scene, dirty_dispatch_plan.take_material_dispatch());
        let material_events = DirtyRouterHelper::events_from_material_update_result(material_result);
        DirtyRouterHelper::route_stage(
            DirtyStageKind::AfterMaterial,
            &material_events,
            scene,
            &mut dirty_dispatch_plan,
        );
        self.render_material_manager.upload(
            resource_ctx,
            cmd,
            transfer_barrier_mask,
            frame_label,
            scene,
            &self.render_texture_manager,
        );

        // instance 阶段消费 dirty dispatch，而不是直接解释 `SceneChanges`。只有 mesh 与
        // material 都解析成功的实例会进入 active 列表。
        let (scene_render_data, instance_result) = self.render_instance_manager.prepare_render_data(
            scene,
            dirty_dispatch_plan.take_instance_dispatch(),
            &self.render_material_manager,
            &self.render_mesh_manager,
        );
        let instance_events = DirtyRouterHelper::events_from_instance_update_result(instance_result);
        DirtyRouterHelper::route_stage(
            DirtyStageKind::AfterInstance,
            &instance_events,
            scene,
            &mut dirty_dispatch_plan,
        );

        if dirty_dispatch_plan.take_analytic_dirty() {
            self.render_analytic_light_manager.mark_dirty();
        }
        let analytic_light_update = self.render_analytic_light_manager.update_and_upload(
            resource_ctx,
            cmd,
            transfer_barrier_mask,
            frame_label,
            scene,
        );
        let _analytic_light_changed = analytic_light_update.changed;

        if dirty_dispatch_plan.take_emissive_dirty() {
            self.render_emissive_light_table.mark_dirty();
        }
        if dirty_dispatch_plan.take_tlas_dirty() {
            self.render_tlas_manager.mark_dirty();
        }

        let material_buffer_device_address = self.render_material_manager.material_buffer_device_address(frame_label);
        let emissive_light_binding = self.render_emissive_light_table.update_and_upload(
            resource_ctx,
            cmd,
            transfer_barrier_mask,
            frame_label,
            &scene_render_data,
            scene,
        );
        Self::upload_render_data(
            &mut self.render_world_buffers,
            &mut self.render_tlas_manager,
            &mut self.raster_draws,
            resource_ctx,
            device_ctx,
            immediate_ctx,
            cmd,
            transfer_barrier_mask,
            frame_id,
            frame_label,
            &scene_render_data,
            material_buffer_device_address,
            environment_binding,
            analytic_light_update.binding,
            emissive_light_binding,
        );

        RenderWorldPrepareResult {
            sky_changed: sky_update.changed,
        }
    }
}

// 只读访问器：供 runtime 内部和 `RenderSceneView` 暴露当前 FIF 的 scene 表示。
impl RenderWorld {
    /// 返回当前 frame label 的 TLAS owner。
    ///
    /// 没有 active instance 时返回 None，render pass 应据此跳过 ray tracing 路径或使用空场景策略。
    #[inline]
    pub fn tlas(&self, frame_label: FrameLabel) -> Option<&GfxAcceleration> {
        self.render_tlas_manager.tlas(frame_label)
    }

    /// 返回当前 frame label 的 scene root buffer。
    ///
    /// buffer 内保存 shader 查找整套 scene 数据所需的 device address、bindless handle 和计数。
    #[inline]
    pub fn scene_buffer(&self, frame_label: FrameLabel) -> &GfxStructuredBuffer<gpu::engine::scene::GpuScene> {
        &self.render_world_buffers[*frame_label].scene_buffer
    }
}

// Render pass 可见契约：隐藏 `RenderWorld` owner，只暴露 scene root、TLAS 与 draw 录制能力。
impl RenderSceneView for RenderWorld {
    /// 暴露 scene root buffer device address，供 shader 通过全局 descriptor 间接读取场景。
    fn scene_buffer_device_address(&self, frame_label: FrameLabel) -> vk::DeviceAddress {
        self.scene_buffer(frame_label).device_address()
    }

    /// 暴露当前 frame label 的 TLAS handle；空场景没有 TLAS。
    fn tlas_handle(&self, frame_label: FrameLabel) -> Option<vk::AccelerationStructureKHR> {
        self.tlas(frame_label).map(|tlas| tlas.handle())
    }

    fn accum_signature(&self, frame_label: FrameLabel) -> RenderSceneAccumSignature {
        self.render_world_buffers[*frame_label].accum_signature
    }

    /// 遍历 prepare 阶段展开好的 raster draw cache 并录制 draw。
    ///
    /// `before_draw` 是 pass 注入点，用于按 instance slot/submesh index 更新 push constants
    /// 或其它 per-draw 状态，同时不暴露 `RenderWorld` 的内部缓存结构。
    fn draw_raster(&self, frame_label: FrameLabel, cmd: &GfxCommandBuffer, before_draw: &mut dyn FnMut(u32, u32)) {
        let _span = tracy_client::span!("RenderWorld::draw_raster");
        draw_raster_cache(&self.raster_draws[*frame_label], cmd, before_draw);
    }
}

// 选择轮廓使用的窄接口：App 提供 CPU World 语义，runtime 负责解析当前 GPU draw。
impl WorldSubmeshRasterView for RenderWorld {
    fn draw_selected_submesh_raster(
        &self,
        frame_label: FrameLabel,
        cmd: &GfxCommandBuffer,
        selection: WorldSubmeshSelection,
        before_draw: &mut dyn FnMut(u32, u32),
    ) -> bool {
        let _span = tracy_client::span!("RenderWorld::draw_selected_submesh_raster");
        let Some((instance_slot, submesh_idx)) =
            self.render_instance_manager.resolve_active_raster_submesh(selection.instance, selection.submesh_index)
        else {
            return false;
        };

        draw_raster_cache_selected(&self.raster_draws[*frame_label], cmd, instance_slot, submesh_idx, before_draw)
    }
}

// Prepare 上传：把 `RenderData` 写入当前 FIF 的 GPU buffer，并刷新本帧可见的 scene root。
impl RenderWorld {
    /// # 阶段：Before Render
    ///
    /// 将 runtime bridge 已经整理好的 `RenderData` 写入当前 FIF 的 device buffer。
    /// 此方法不依赖 `SceneStore`，这是 CPU scene 与 GPU scene 的边界。
    ///
    /// 上传顺序刻意保持为 draw cache、mesh/instance/light buffer、TLAS、scene root buffer：
    /// scene root buffer 最后写入，确保它记录的 device address 与本帧实际 buffer/TLAS 对齐。
    fn upload_render_data(
        render_world_buffers: &mut [RenderWorldBuffers; FrameLabel::COUNT],
        render_tlas_manager: &mut RenderTlasManager,
        raster_draws: &mut [Vec<RasterDrawItem>; FrameLabel::COUNT],
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        cmd: &GfxCommandBuffer,
        barrier_mask: GfxBarrierMask,
        frame_id: u64,
        frame_label: FrameLabel,
        render_data: &RenderData<'_>,
        material_buffer_device_address: vk::DeviceAddress,
        environment_binding: EnvironmentBinding,
        analytic_light_binding: AnalyticLightBinding,
        emissive_light_binding: EmissiveLightBinding,
    ) {
        let _span = tracy_client::span!("RenderWorld::prepare_render_data");

        update_raster_draw_cache(&mut raster_draws[*frame_label], render_data);
        Self::upload_mesh_buffer(render_world_buffers, resource_ctx, cmd, barrier_mask, render_data, frame_label);
        Self::upload_instance_buffer(render_world_buffers, resource_ctx, cmd, barrier_mask, render_data, frame_label);

        // TLAS instance 描述使用稳定 instance slot 与 transform，因此必须在 instance buffer
        // 写入逻辑之后构建，保证 GPU scene buffer、TLAS custom index 和 raster draw cache 对齐。
        render_tlas_manager.build_or_update(
            resource_ctx,
            device_ctx,
            immediate_ctx,
            render_data,
            frame_id,
            frame_label,
        );
        let current_tlas_revision = render_tlas_manager.tlas_revision(frame_label);

        Self::upload_scene_buffer(
            render_world_buffers,
            cmd,
            frame_label,
            barrier_mask,
            material_buffer_device_address,
            current_tlas_revision,
            environment_binding,
            analytic_light_binding,
            emissive_light_binding,
        );
    }

    /// 将 GPU scene root 数据上传到 scene buffer。
    ///
    /// 这个 buffer 只保存 device address、bindless handle 和计数，是 shader 访问整套场景数据的入口。
    fn upload_scene_buffer(
        render_world_buffers: &mut [RenderWorldBuffers; FrameLabel::COUNT],
        cmd: &GfxCommandBuffer,
        frame_label: FrameLabel,
        barrier_mask: GfxBarrierMask,
        material_buffer_device_address: vk::DeviceAddress,
        tlas_revision: u64,
        environment_binding: EnvironmentBinding,
        analytic_light_binding: AnalyticLightBinding,
        emissive_light_binding: EmissiveLightBinding,
    ) {
        let frame_index = *frame_label;
        let crt_gpu_buffers = &render_world_buffers[frame_index];
        // scene root buffer 只存放“入口地址”和资源句柄，不复制大块 scene 数据。
        // 它最后写入，确保地址/count 与本帧刚上传的 buffer 和 TLAS revision 匹配。
        let gpu_scene_data = gpu::engine::scene::GpuScene {
            all_instances: crt_gpu_buffers.instance_buffer.device_address(),
            all_mats: material_buffer_device_address,
            all_geometries: crt_gpu_buffers.geometry_buffer.device_address(),
            instance_material_map: crt_gpu_buffers.material_indirect_buffer.device_address(),
            instance_geometry_map: crt_gpu_buffers.geometry_indirect_buffer.device_address(),
            point_lights: analytic_light_binding.point_lights,
            spot_lights: analytic_light_binding.spot_lights,
            area_lights: analytic_light_binding.area_lights,
            emissive_triangle_lights: emissive_light_binding.triangle_lights_device_address,
            emissive_light_alias_table: emissive_light_binding.alias_table_device_address,
            instance_emissive_triangle_base_map: emissive_light_binding.base_map_device_address,
            emissive_light_count: emissive_light_binding.alias_count,
            emissive_light_enabled: emissive_light_binding.enabled,
            emissive_light_version: emissive_light_binding.version,
            emissive_light_record_count: emissive_light_binding.record_count,
            point_light_count: analytic_light_binding.point_light_count,
            spot_light_count: analytic_light_binding.spot_light_count,
            area_light_count: analytic_light_binding.area_light_count,
            analytic_light_version: analytic_light_binding.version,

            sky: environment_binding.sky.srv_handle.0,
            sky_sampler_type: environment_binding.sky.sampler,
            sky_distribution: environment_binding.sky.distribution_device_address,
            sky_distribution_width: environment_binding.sky.distribution_width,
            sky_distribution_height: environment_binding.sky.distribution_height,
            sky_distribution_enabled: environment_binding.sky.distribution_enabled,
            sky_distribution_version: environment_binding.sky.distribution_version,
        };

        cmd.cmd_update_buffer(crt_gpu_buffers.scene_buffer.vk_buffer(), 0, BytesConvert::bytes_of(&gpu_scene_data));
        cmd.buffer_memory_barrier(
            vk::DependencyFlags::empty(),
            &[GfxBufferBarrier::default().mask(barrier_mask).buffer(
                crt_gpu_buffers.scene_buffer.vk_buffer(),
                0,
                vk::WHOLE_SIZE,
            )],
        );

        // 累计签名必须在本帧 scene root buffer、TLAS、light 与 sky binding 都确定后写入。
        // 它只描述会让离线 reference 历史失效的语义版本，不暴露具体 GPU buffer 所有权。
        let accum_signature = RenderSceneAccumSignature {
            tlas_revision,
            emissive_light_version: emissive_light_binding.version,
            analytic_light_version: analytic_light_binding.version,
            sky_distribution_version: environment_binding.sky.distribution_version,
        };
        render_world_buffers[frame_index].accum_signature = accum_signature;
    }

    /// 将 mesh 数据以 geometry 表的形式上传到 GPU。
    ///
    /// geometry 表只保存 device address；实际 vertex/index buffer 生命周期由 mesh manager 持有。
    fn upload_mesh_buffer(
        render_world_buffers: &mut [RenderWorldBuffers; FrameLabel::COUNT],
        resource_ctx: GfxResourceCtx<'_>,
        cmd: &GfxCommandBuffer,
        barrier_mask: GfxBarrierMask,
        scene_data: &RenderData<'_>,
        frame_label: FrameLabel,
    ) {
        let _span = tracy_client::span!("upload_mesh_buffer2");
        let crt_gpu_buffers = &mut render_world_buffers[*frame_label];
        let crt_geometry_stage_buffer = &mut crt_gpu_buffers.geometry_stage_buffer;
        let geometry_buffer_slices = crt_geometry_stage_buffer.mapped_slice();

        let mut crt_geometry_idx = 0;
        for mesh in scene_data.all_meshes.iter() {
            // RenderData 已经按 mesh 去重；这里把每个 mesh 的 submesh 展开为全局 geometry table，
            // instance 只通过 geometry_indirect_buffer 指向其中一段连续范围。
            if geometry_buffer_slices.len() < crt_geometry_idx + mesh.geometries.len() {
                panic!("geometry cnt can not be larger than buffer");
            }
            for (submesh_idx, geometry) in mesh.geometries.iter().enumerate() {
                geometry_buffer_slices[crt_geometry_idx + submesh_idx] = gpu::engine::geometry::Geometry {
                    position_buffer: geometry.vertex_buffer.pos_address(),
                    normal_buffer: geometry.vertex_buffer.normal_address(),
                    tangent_buffer: geometry.vertex_buffer.tangent_address(),
                    uv_buffer: geometry.vertex_buffer.uv_address(),
                    index_buffer: geometry.index_buffer.device_address(),
                };
            }
            crt_geometry_idx += mesh.geometries.len();
        }

        flush_copy_and_barrier(
            resource_ctx,
            cmd,
            crt_geometry_stage_buffer,
            &mut crt_gpu_buffers.geometry_buffer,
            barrier_mask,
        );
    }

    /// 将 instance 数据上传到 GPU。
    ///
    /// `instance_slot` 是全局稳定 slot；geometry/material indirect buffer 则是本帧紧凑列表，
    /// 用于把一个 instance 映射到它的 submesh geometry 与 material slot。
    fn upload_instance_buffer(
        render_world_buffers: &mut [RenderWorldBuffers; FrameLabel::COUNT],
        resource_ctx: GfxResourceCtx<'_>,
        cmd: &GfxCommandBuffer,
        barrier_mask: GfxBarrierMask,
        scene_data: &RenderData<'_>,
        frame_label: FrameLabel,
    ) {
        let _span = tracy_client::span!("upload_instance_buffer2");
        let crt_gpu_buffers = &mut render_world_buffers[*frame_label];

        let crt_instance_stage_buffer = &mut crt_gpu_buffers.instance_stage_buffer;
        let crt_geometry_indirect_stage_buffer = &mut crt_gpu_buffers.geometry_indirect_stage_buffer;
        let crt_material_indirect_stage_buffer = &mut crt_gpu_buffers.material_indirect_stage_buffer;

        let instance_buffer_slices = crt_instance_stage_buffer.mapped_slice();
        let material_indirect_buffer_slices = crt_material_indirect_stage_buffer.mapped_slice();
        let geometry_indirect_buffer_slices = crt_geometry_indirect_stage_buffer.mapped_slice();

        let mut crt_geometry_indirect_idx = 0;
        let mut crt_material_indirect_idx = 0;
        for instance in scene_data.all_instances.iter() {
            // instance buffer 使用全局稳定 slot 下标写入；indirect buffer 使用本帧紧凑下标写入。
            // 这种布局让 shader 可以先用 TLAS/raster 提供的 instance slot 找到 instance，
            // 再通过 instance 中的 indirect range 找到对应 submesh 列表。
            let instance_slot = instance.instance_slot.as_usize();
            if instance_buffer_slices.len() <= instance_slot {
                panic!("instance slot can not be larger than buffer");
            }

            let mesh = &scene_data.all_meshes[instance.mesh_index];
            let submesh_cnt = instance.material_slots.len();
            debug_assert_eq!(
                mesh.geometries.len(),
                submesh_cnt,
                "instance material slots must stay aligned with mesh geometry count"
            );
            if geometry_indirect_buffer_slices.len() < crt_geometry_indirect_idx + submesh_cnt {
                panic!("instance geometry cnt can not be larger than buffer");
            }
            if material_indirect_buffer_slices.len() < crt_material_indirect_idx + submesh_cnt {
                panic!("instance material cnt can not be larger than buffer");
            }

            instance_buffer_slices[instance_slot] = gpu::engine::scene::Instance {
                geometry_indirect_idx: crt_geometry_indirect_idx as u32,
                geometry_count: submesh_cnt as u32,
                material_indirect_idx: crt_material_indirect_idx as u32,
                material_count: submesh_cnt as u32,
                model: instance.transform.into(),
                inv_model: instance.transform.inverse().into(),
                prev_model: instance.previous_transform.into(),
            };

            // 将 geometry 索引写入间接索引 buffer。
            // mesh 在 RenderData 中去重，instance 只保存它引用的 submesh 范围。
            let mesh_startup_index = scene_data.mesh_geometry_start_indices[instance.mesh_index];
            for submesh_idx in 0..submesh_cnt {
                let geometry_idx = mesh_startup_index + submesh_idx;
                geometry_indirect_buffer_slices[crt_geometry_indirect_idx + submesh_idx] = geometry_idx as u32;
            }
            crt_geometry_indirect_idx += submesh_cnt;

            // 将稳定 material slot 写入间接索引 buffer。
            // material slot 来自 RenderMaterialManager，可被 shader 直接索引材质 buffer。
            for material_slot in instance.material_slots.iter() {
                material_indirect_buffer_slices[crt_material_indirect_idx] = *material_slot;
                crt_material_indirect_idx += 1;
            }
        }

        flush_copy_and_barrier(
            resource_ctx,
            cmd,
            crt_instance_stage_buffer,
            &mut crt_gpu_buffers.instance_buffer,
            barrier_mask,
        );
        flush_copy_and_barrier(
            resource_ctx,
            cmd,
            crt_geometry_indirect_stage_buffer,
            &mut crt_gpu_buffers.geometry_indirect_buffer,
            barrier_mask,
        );
        flush_copy_and_barrier(
            resource_ctx,
            cmd,
            crt_material_indirect_stage_buffer,
            &mut crt_gpu_buffers.material_indirect_buffer,
            barrier_mask,
        );
    }
}

/// 三个操作：
/// 1. 将 stage buffer 的数据 *全部* flush 到 buffer 中
/// 2. 从 stage buffer 中将 *所有* 数据复制到目标 buffer 中
/// 3. 添加 barrier，确保后续访问时 Copy 已经完成且数据可用
///
/// 当前 scene buffer 上传采用整 buffer copy，简化 dirty tracking；调用者负责传入后续 shader
/// 阶段需要的可见性 mask。
fn flush_copy_and_barrier(
    resource_ctx: GfxResourceCtx<'_>,
    cmd: &GfxCommandBuffer,
    stage_buffer: &mut GfxBuffer,
    dst: &mut GfxBuffer,
    barrier_mask: GfxBarrierMask,
) {
    let buffer_size = stage_buffer.size();
    stage_buffer.flush(resource_ctx, 0, buffer_size);
    cmd.cmd_copy_buffer(
        stage_buffer,
        dst,
        &[vk::BufferCopy {
            size: buffer_size,
            ..Default::default()
        }],
    );
    cmd.buffer_memory_barrier(
        vk::DependencyFlags::empty(),
        &[GfxBufferBarrier::default().mask(barrier_mask).buffer(dst.vk_buffer(), 0, vk::WHOLE_SIZE)],
    );
}
