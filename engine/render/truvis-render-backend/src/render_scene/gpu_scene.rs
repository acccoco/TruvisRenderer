use ash::vk;

use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_gfx::gfx::{GfxDeviceCtx, GfxImmediateCtx, GfxResourceCtx};
use truvis_gfx::raytracing::acceleration::GfxAcceleration;
use truvis_gfx::resources::special_buffers::structured_buffer::GfxStructuredBuffer;
use truvis_render_interface::bindless_manager::BindlessManager;
use truvis_render_interface::frame_counter::FrameCounter;
use truvis_render_interface::gfx_resource_manager::GfxResourceManager;
use truvis_render_interface::pipeline_settings::FrameLabel;
use truvis_render_interface::render_scene_view::RenderSceneView;
use truvis_shader_binding::gpu;

use super::buffers::GpuSceneBuffers;
use super::default_environment::DefaultEnvironment;
use super::raster_draw_cache::{RasterDrawItem, draw_raster_cache};

/// backend 私有的 GPU scene 翻译层。
///
/// 它把 `InstanceBridge` 产出的 `RenderData` 转换成 shader 可读的 GPU buffer、TLAS 和
/// 光栅化 draw cache，并通过 `RenderSceneView` 向 render pass 暴露只读能力。
/// `GpuScene` 不拥有 CPU scene；它只保存当前 FIF 可用的 GPU 表示。
pub struct GpuScene {
    pub(super) gpu_scene_buffers: [GpuSceneBuffers; FrameCounter::fif_count()],
    pub(super) raster_draws: [Vec<RasterDrawItem>; FrameCounter::fif_count()],
    pub(super) environment: DefaultEnvironment,
}

// 访问器
impl GpuScene {
    #[inline]
    pub fn tlas(&self, frame_label: FrameLabel) -> Option<&GfxAcceleration> {
        self.gpu_scene_buffers[*frame_label].tlas.as_ref()
    }

    #[inline]
    pub fn scene_buffer(&self, frame_label: FrameLabel) -> &GfxStructuredBuffer<gpu::GPUScene> {
        &self.gpu_scene_buffers[*frame_label].scene_buffer
    }
}

// 创建与初始化
impl GpuScene {
    pub fn new(
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        gfx_resource_manager: &mut GfxResourceManager,
        bindless_manager: &mut BindlessManager,
    ) -> Self {
        let _span = tracy_client::span!("GpuScene::new");

        let environment = {
            let _span = tracy_client::span!("GpuScene::new/default_environment");
            DefaultEnvironment::new(resource_ctx, device_ctx, immediate_ctx, gfx_resource_manager, bindless_manager)
        };

        let gpu_scene_buffers = {
            let _span = tracy_client::span!("GpuScene::new/per_frame_buffers");
            FrameCounter::frame_labes().map(|frame_label| GpuSceneBuffers::new(resource_ctx, frame_label))
        };

        Self {
            gpu_scene_buffers,
            raster_draws: FrameCounter::frame_labes().map(|_| Vec::new()),
            environment,
        }
    }
}

// 销毁
impl GpuScene {
    pub fn destroy_mut(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        bindless_manager: &mut BindlessManager,
        gfx_resource_manager: &mut GfxResourceManager,
    ) {
        for buffers in &mut self.gpu_scene_buffers {
            buffers.destroy_mut(resource_ctx, device_ctx);
        }

        self.environment.destroy_mut(resource_ctx, device_ctx, bindless_manager, gfx_resource_manager);
    }
}

impl RenderSceneView for GpuScene {
    fn scene_buffer_device_address(&self, frame_label: FrameLabel) -> vk::DeviceAddress {
        self.scene_buffer(frame_label).device_address()
    }

    fn tlas_handle(&self, frame_label: FrameLabel) -> Option<vk::AccelerationStructureKHR> {
        self.tlas(frame_label).map(|tlas| tlas.handle())
    }

    fn draw_raster(&self, frame_label: FrameLabel, cmd: &GfxCommandBuffer, before_draw: &mut dyn FnMut(u32, u32)) {
        let _span = tracy_client::span!("GpuScene::draw_raster");
        draw_raster_cache(&self.raster_draws[*frame_label], cmd, before_draw);
    }
}
