use ash::vk;

use truvis_asset::asset_hub::{AssetHub, AssetLoadedEvent};
use truvis_gfx::basic::bytes::BytesConvert;
use truvis_gfx::commands::barrier::{GfxBarrierMask, GfxBufferBarrier};
use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_gfx::commands::submit_info::GfxSubmitInfo;
use truvis_gfx::gfx::Gfx;
use truvis_render_interface::render_world::RenderWorld;
use truvis_shader_binding::gpu;
use truvis_world::World;

use crate::asset_mesh_uploader::AssetMeshUploader;
use crate::asset_texture_uploader::AssetTextureUploader;
use crate::instance_bridge::InstanceBridge;
use crate::material_bridge::MaterialBridge;
use crate::platform::camera::Camera;
use crate::platform::timer::Timer;
use crate::render_scene::gpu_scene::GpuScene;

/// 每帧 asset loaded 事件到 render-side uploader 的分发阶段。
///
/// `RenderBackend::begin_frame` 只负责推进帧生命周期；具体 texture/mesh 事件如何进入
/// uploader 由这个 stage 内聚，避免 backend owner 直接承载 asset 事件分类细节。
pub(crate) struct AssetUploadStage;

impl AssetUploadStage {
    /// 消费 `AssetHub::update` 产出的加载事件，并转发给对应 render-side uploader。
    ///
    /// texture 与 mesh 事件会进入 GPU 上传队列；scene 事件只表示 CPU 数据可被 scene 层读取，
    /// 具体实例化仍由 app/scene manager 控制，不在 backend 自动创建运行时实例。
    pub(crate) fn update(
        asset_hub: &mut AssetHub,
        asset_texture_uploader: &mut AssetTextureUploader,
        asset_mesh_uploader: &mut AssetMeshUploader,
        gfx: &Gfx,
        render_world: &mut RenderWorld,
    ) {
        let loaded_asset_events = asset_hub.update();
        let mut texture_events = Vec::new();
        let mut mesh_events = Vec::new();
        for event in loaded_asset_events {
            // 事件分流集中在这里，避免 RenderBackend 生命周期入口直接知道每种 asset
            // 对应的 uploader 细节，也让 uploader 可以用更窄的事件集合维护自身契约。
            match event {
                event @ (AssetLoadedEvent::TextureLoaded { .. } | AssetLoadedEvent::TextureFailed { .. }) => {
                    texture_events.push(event);
                }
                event @ AssetLoadedEvent::MeshLoaded { .. } => {
                    mesh_events.push(event);
                }
                AssetLoadedEvent::SceneLoaded { handle } => {
                    log::debug!("Scene asset {:?} CPU data is ready", handle);
                }
                AssetLoadedEvent::SceneFailed { handle, error } => {
                    log::error!("Scene asset {:?} failed to load: {}", handle, error);
                }
            }
        }

        asset_texture_uploader.update(
            texture_events,
            gfx.resource_ctx(),
            gfx.device_ctx(),
            gfx.queue_ctx(),
            &mut render_world.gfx_resource_manager,
            &mut render_world.bindless_manager,
        );
        asset_mesh_uploader.update(mesh_events, gfx.resource_ctx(), gfx.device_ctx(), gfx.queue_ctx());
    }
}

/// `PreparePipeline` 的一次性借用集合。
///
/// 该上下文只在 update 与 render 之间短暂存在，集中表达 prepare 阶段需要读写的边界：
/// CPU `World` 只读，render-side bridge/GpuScene 可变，具体 app-owned camera 只读。
pub(crate) struct PreparePipelineCtx<'a> {
    /// backend root owner，用于提交 prepare 命令和访问 typed Vulkan/GPU 上下文。
    pub(crate) gfx: &'a Gfx,
    /// CPU 语义世界；prepare 只读取场景、资产和灯光快照，不在此阶段修改 app 状态。
    pub(crate) world: &'a World,
    /// GPU frame state 与 descriptor/manager 集合；prepare 会写入当前 FIF 对应资源。
    pub(crate) render_world: &'a mut RenderWorld,
    /// shader texture binding 查询源；未 ready 的 texture 会通过 fallback 解析。
    pub(crate) asset_texture_uploader: &'a AssetTextureUploader,
    /// GPU-ready mesh/BLAS 查询源；未 ready 的 mesh 会让实例保持 pending。
    pub(crate) asset_mesh_uploader: &'a AssetMeshUploader,
    /// asset material 到稳定 GPU material slot 的桥接层。
    pub(crate) material_bridge: &'a mut MaterialBridge,
    /// runtime instance 到稳定 GPU instance slot 的桥接层。
    pub(crate) instance_bridge: &'a mut InstanceBridge,
    /// backend 私有 GPU scene 翻译层。
    pub(crate) gpu_scene: &'a mut GpuScene,
    /// 每帧 uniform 中的时间数据来源。
    pub(crate) timer: &'a Timer,
    /// 当前 FIF 的 GPU scene 更新命令缓冲。
    pub(crate) cmd: &'a GfxCommandBuffer,
    /// app 提供的相机快照；backend 不持有相机状态。
    pub(crate) camera: &'a Camera,
}

/// Update 和 render 之间的固定 prepare pipeline。
///
/// 它把 CPU `World`、asset/material/instance bridge 的状态翻译为当前 FIF 可见的 GPU scene
/// 与 per-frame uniform。`RenderBackend` 仍拥有资源生命周期，但不直接展开每个 prepare 子步骤。
pub(crate) struct PreparePipeline;

impl PreparePipeline {
    /// 执行完整 prepare 阶段。
    ///
    /// 调用点必须位于 update 之后、render graph 录制之前；该函数会把 CPU scene/material/asset
    /// 状态固化成当前 FIF 的 GPU buffer、TLAS、descriptor 和 per-frame uniform。
    pub(crate) fn prepare(ctx: PreparePipelineCtx<'_>) {
        let _span = tracy_client::span!("PreparePipeline::prepare");
        // 累积渲染只关心相机方向和位置是否变化。这里使用 app camera 快照更新计数，
        // 让后续 pass 可以决定是否复用上一帧累积结果。
        let current_camera_dir =
            glam::vec3(ctx.camera.euler_yaw_deg, ctx.camera.euler_pitch_deg, ctx.camera.euler_roll_deg);
        ctx.render_world.accum_data.update_accum_frames(current_camera_dir, ctx.camera.position);

        Self::prepare_gpu_scene(ctx);
    }

    /// 合成 `GpuScene` 用于判断 TLAS 是否过期的 scene revision。
    ///
    /// mesh ready revision 覆盖 BLAS 新增/替换，instance revision 覆盖实例增删、ready 状态
    /// 和 transform 变化；使用 saturating add 保证长时间运行时不会回绕成旧 revision。
    pub(crate) fn scene_revision(mesh_ready_revision: u64, instance_revision: u64) -> u64 {
        mesh_ready_revision.saturating_add(instance_revision)
    }

    /// 准备 render pass 可见的 GPU scene 与 per-frame uniform。
    ///
    /// 该函数把所有 staging copy 录到同一个 command buffer，最后一次提交到 graphics queue；
    /// render graph 在后续命令提交中通过常规 queue 顺序看到这些写入。
    fn prepare_gpu_scene(ctx: PreparePipelineCtx<'_>) {
        let _span = tracy_client::span!("PreparePipeline::prepare_gpu_scene");
        let frame_extent = ctx.render_world.frame_settings.frame_extent;
        let frame_label = ctx.render_world.frame_counter.frame_label();

        // GPU scene 更新使用独立命令缓冲，把 material/instance/geometry/light/scene buffer
        // 的 staging copy 和 barrier 串在一起，作为 render graph 录制前的固定准备阶段。
        ctx.cmd.begin(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT, "[update-draw-buffer]stage-to-ubo");

        let transfer_barrier_mask = GfxBarrierMask {
            src_stage: vk::PipelineStageFlags2::TRANSFER,
            src_access: vk::AccessFlags2::TRANSFER_WRITE,
            dst_stage: vk::PipelineStageFlags2::VERTEX_SHADER
                | vk::PipelineStageFlags2::FRAGMENT_SHADER
                | vk::PipelineStageFlags2::RAY_TRACING_SHADER_KHR
                | vk::PipelineStageFlags2::COMPUTE_SHADER,
            dst_access: vk::AccessFlags2::SHADER_READ | vk::AccessFlags2::UNIFORM_READ,
        };

        let bindless_target = ctx.render_world.global_descriptor_sets.bindless_target();
        // bindless 表先更新，因为 material upload 可能立即解析 texture SRV handle；
        // 后续 scene root buffer 也会写入默认环境贴图的 bindless handle。
        ctx.render_world.bindless_manager.prepare_render_data(
            ctx.gfx.device_ctx(),
            &ctx.render_world.gfx_resource_manager,
            bindless_target,
        );

        // material 阶段以 AssetHub 为事实来源同步稳定 slot，再根据 texture ready/fallback
        // 状态写当前 FIF 的 material buffer。
        ctx.material_bridge.sync_asset_materials(&ctx.world.asset_hub);
        ctx.material_bridge.update_textures(ctx.asset_texture_uploader);
        ctx.material_bridge.upload(
            ctx.gfx.resource_ctx(),
            ctx.cmd,
            transfer_barrier_mask,
            frame_label,
            ctx.asset_texture_uploader,
        );

        // instance 阶段是 CPU scene 到 render-side `RenderData` 的边界；只有 mesh 与 material
        // 都解析成功的实例会进入 active 列表。
        let scene_render_data = ctx.instance_bridge.prepare_render_data(
            &ctx.world.scene_manager,
            ctx.material_bridge,
            ctx.asset_mesh_uploader,
        );
        let material_buffer_device_address = ctx.material_bridge.material_buffer_device_address(frame_label);
        // mesh ready 与 instance 变化都会影响 TLAS；两个 revision 合成一条 scene revision，
        // 交给 GpuScene 判断当前 FIF 的 TLAS 是否需要重建。
        let scene_revision =
            Self::scene_revision(ctx.asset_mesh_uploader.ready_revision(), ctx.instance_bridge.revision());
        ctx.gpu_scene.upload_render_data(
            ctx.gfx.resource_ctx(),
            ctx.gfx.device_ctx(),
            ctx.gfx.immediate_ctx(),
            ctx.cmd,
            transfer_barrier_mask,
            &ctx.render_world.frame_counter,
            &scene_render_data,
            material_buffer_device_address,
            scene_revision,
            &ctx.render_world.bindless_manager,
        );

        let view = ctx.camera.get_view_matrix();
        let projection = ctx.camera.get_projection_matrix();
        // per-frame uniform 放在 GPU scene 上传之后写入同一条命令缓冲，保证本帧 shader
        // 看到的相机、分辨率、时间和 scene buffer 都来自同一个 prepare 快照。
        let per_frame_data = gpu::PerFrameData {
            projection: projection.into(),
            view: view.into(),
            inv_view: view.inverse().into(),
            inv_projection: projection.inverse().into(),
            camera_pos: ctx.camera.position.into(),
            camera_forward: ctx.camera.camera_forward().into(),
            time_ms: ctx.timer.total_time_ms(),
            delta_time_ms: ctx.timer.delta_time_ms(),
            frame_id: ctx.render_world.frame_counter.frame_id(),
            resolution: gpu::Float2 {
                x: frame_extent.width as f32,
                y: frame_extent.height as f32,
            },
            accum_frames: ctx.render_world.accum_data.accum_frames_num() as u32,
            _padding_0: Default::default(),
            _padding_1: Default::default(),
            _padding_2: Default::default(),
        };
        let crt_frame_data_buffer = &ctx.render_world.per_frame_data_buffers[*frame_label];
        ctx.cmd.cmd_update_buffer(crt_frame_data_buffer.vk_buffer(), 0, BytesConvert::bytes_of(&per_frame_data));
        ctx.cmd.buffer_memory_barrier(
            vk::DependencyFlags::empty(),
            &[GfxBufferBarrier::default()
                .buffer(crt_frame_data_buffer.vk_buffer(), 0, vk::WHOLE_SIZE)
                .mask(transfer_barrier_mask)],
        );
        ctx.cmd.end();
        ctx.gfx.queue_ctx().gfx_queue().submit(vec![GfxSubmitInfo::new(std::slice::from_ref(ctx.cmd))], None);
    }
}

#[cfg(test)]
mod tests {
    use super::PreparePipeline;

    #[test]
    fn scene_revision_combines_mesh_and_instance_revisions() {
        assert_eq!(PreparePipeline::scene_revision(3, 7), 10);
    }

    #[test]
    fn scene_revision_saturates_on_overflow() {
        assert_eq!(PreparePipeline::scene_revision(u64::MAX, 1), u64::MAX);
    }
}
