use ash::vk;

use truvis_asset::asset_hub::{AssetHub, LoadedAssetEvent};
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
            match event {
                event @ (LoadedAssetEvent::TextureLoaded { .. } | LoadedAssetEvent::TextureFailed { .. }) => {
                    texture_events.push(event);
                }
                event @ LoadedAssetEvent::MeshLoaded { .. } => {
                    mesh_events.push(event);
                }
                LoadedAssetEvent::SceneLoaded { handle } => {
                    log::debug!("Scene asset {:?} CPU data is ready", handle);
                }
                LoadedAssetEvent::SceneFailed { handle, error } => {
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

pub(crate) struct PreparePipelineCtx<'a> {
    pub(crate) gfx: &'a Gfx,
    pub(crate) world: &'a World,
    pub(crate) render_world: &'a mut RenderWorld,
    pub(crate) asset_texture_uploader: &'a AssetTextureUploader,
    pub(crate) asset_mesh_uploader: &'a AssetMeshUploader,
    pub(crate) material_bridge: &'a mut MaterialBridge,
    pub(crate) instance_bridge: &'a mut InstanceBridge,
    pub(crate) gpu_scene: &'a mut GpuScene,
    pub(crate) timer: &'a Timer,
    pub(crate) cmd: &'a GfxCommandBuffer,
    pub(crate) camera: &'a Camera,
}

/// Update 和 render 之间的固定 prepare pipeline。
///
/// 它把 CPU `World`、asset/material/instance bridge 的状态翻译为当前 FIF 可见的 GPU scene
/// 与 per-frame uniform。`RenderBackend` 仍拥有资源生命周期，但不直接展开每个 prepare 子步骤。
pub(crate) struct PreparePipeline;

impl PreparePipeline {
    pub(crate) fn prepare(ctx: PreparePipelineCtx<'_>) {
        let _span = tracy_client::span!("PreparePipeline::prepare");
        let current_camera_dir =
            glam::vec3(ctx.camera.euler_yaw_deg, ctx.camera.euler_pitch_deg, ctx.camera.euler_roll_deg);
        ctx.render_world.accum_data.update_accum_frames(current_camera_dir, ctx.camera.position);

        Self::prepare_gpu_scene(ctx);
    }

    pub(crate) fn scene_revision(mesh_ready_revision: u64, instance_revision: u64) -> u64 {
        mesh_ready_revision.saturating_add(instance_revision)
    }

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
        ctx.render_world.bindless_manager.prepare_render_data(
            ctx.gfx.device_ctx(),
            &ctx.render_world.gfx_resource_manager,
            bindless_target,
        );

        ctx.material_bridge.sync_asset_materials(&ctx.world.asset_hub);
        ctx.material_bridge.update_textures(ctx.asset_texture_uploader);
        ctx.material_bridge.upload(
            ctx.gfx.resource_ctx(),
            ctx.cmd,
            transfer_barrier_mask,
            frame_label,
            ctx.asset_texture_uploader,
        );

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
