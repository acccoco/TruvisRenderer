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

/// runtime 私有的 GPU scene 翻译层。
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
    /// 返回当前 frame label 的 TLAS owner。
    ///
    /// 没有 active instance 时返回 None，render pass 应据此跳过 ray tracing 路径或使用空场景策略。
    #[inline]
    pub fn tlas(&self, frame_label: FrameLabel) -> Option<&GfxAcceleration> {
        self.gpu_scene_buffers[*frame_label].tlas.as_ref()
    }

    /// 返回当前 frame label 的 scene root buffer。
    ///
    /// buffer 内保存 shader 查找整套 scene 数据所需的 device address、bindless handle 和计数。
    #[inline]
    pub fn scene_buffer(&self, frame_label: FrameLabel) -> &GfxStructuredBuffer<gpu::GPUScene> {
        &self.gpu_scene_buffers[*frame_label].scene_buffer
    }
}

// 创建与初始化
impl GpuScene {
    /// 创建默认环境贴图和每个 FIF frame label 的 GPU scene buffer 集。
    ///
    /// `GpuScene` 只创建 render-side 表示，不读取 CPU scene；动态实例和 mesh 数据在
    /// prepare 阶段由 `upload_render_data` 写入。
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
    /// 销毁 GPU scene 拥有的 buffer、TLAS 和默认环境贴图。
    ///
    /// 调用点位于 `RenderRuntime::destroy`，此时 device 已 idle，因此每个 FIF 的 TLAS
    /// 和 buffer 都可以按 shutdown reason 释放。
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
    /// 暴露 scene root buffer device address，供 shader 通过全局 descriptor 间接读取场景。
    fn scene_buffer_device_address(&self, frame_label: FrameLabel) -> vk::DeviceAddress {
        self.scene_buffer(frame_label).device_address()
    }

    /// 暴露当前 frame label 的 TLAS handle；空场景没有 TLAS。
    fn tlas_handle(&self, frame_label: FrameLabel) -> Option<vk::AccelerationStructureKHR> {
        self.tlas(frame_label).map(|tlas| tlas.handle())
    }

    /// 遍历 prepare 阶段展开好的 raster draw cache 并录制 draw。
    ///
    /// `before_draw` 是 pass 注入点，用于按 instance slot/submesh index 更新 push constants
    /// 或其它 per-draw 状态，同时不暴露 `GpuScene` 的内部缓存结构。
    fn draw_raster(&self, frame_label: FrameLabel, cmd: &GfxCommandBuffer, before_draw: &mut dyn FnMut(u32, u32)) {
        let _span = tracy_client::span!("GpuScene::draw_raster");
        draw_raster_cache(&self.raster_draws[*frame_label], cmd, before_draw);
    }
}
