//! 应用插件契约与兼容适配层。
//!
//! [`AppPlugin`] 是新的应用扩展 trait，通过多阶段 hook 表达 update/UI/render 等职责。
//! [`LegacyOuterAppAdapter`] 将旧 [`OuterApp`](crate::outer_app::base::OuterApp) 包装为
//! `AppPlugin`，保证现有 demo 在迁移窗口内平滑运行。

use truvis_gfx::commands::semaphore::GfxSemaphore;
use truvis_renderer::platform::camera::Camera;
use truvis_renderer::renderer::Renderer;

#[allow(deprecated)]
use crate::outer_app::base::OuterApp;

/// 应用插件 trait —— `FrameRuntime` 的帧阶段 hook 契约。
///
/// 每帧按固定顺序调用：
/// `init` (仅一次) → 每帧 `build_ui` → `update` → `render` → …
///
/// `on_resize` 仅在 swapchain 重建成功后、下一帧渲染提交前触发。
/// `shutdown` 在运行时销毁前调用，用于显式资源释放。
///
/// # Renderer 交互边界
///
/// `AppPlugin` 接收 `&Renderer` / `&mut Renderer` 作为上下文，但应遵守以下约束：
///
/// - **稳定接口**：`render_context` 中的 `frame_settings`、`frame_counter`、
///   `pipeline_settings`、`fif_buffers`、`global_descriptor_sets`、`sampler_manager`。
/// - **不应依赖**：`Renderer` 的内部字段布局（如 `gpu_scene_update_cmds`）
///   或 `RenderContext` 中子系统的可变方法（如 `scene_manager` / `asset_hub` 的写入）。
///
/// 后续 change 将通过受控上下文对象进一步收窄此边界。
pub trait AppPlugin {
    /// 初始化渲染相关资源与场景初始状态。
    ///
    /// 在窗口 / surface 创建完毕、`Renderer` 就绪后调用一次。
    fn init(&mut self, renderer: &mut Renderer, camera: &mut Camera);

    /// 每帧 CPU 侧更新入口：输入、相机、业务逻辑。
    ///
    /// 调用时机：phase_input 完成后、GPU 数据准备前。
    fn update(&mut self, renderer: &mut Renderer);

    /// 每帧 UI 构建入口。
    ///
    /// 在 runtime 内置 overlay 绘制之后调用，应用可追加自定义 ImGui 窗口。
    fn build_ui(&mut self, ui: &imgui::Ui);

    /// 每帧 GPU 命令录制与提交入口。
    ///
    /// `gui_draw_data` 已编译完成可直接使用；`timeline` 用于帧间同步。
    fn render(&self, renderer: &Renderer, gui_draw_data: &imgui::DrawData, timeline: &GfxSemaphore);

    /// swapchain 重建完成后触发的资源重建入口。
    ///
    /// 仅在 resize 或 out-of-date 导致 swapchain 重建 **成功** 后、
    /// 下一帧渲染提交前被调用。默认空实现。
    fn on_resize(&mut self, _renderer: &mut Renderer) {}

    /// 运行时销毁前的资源释放入口。
    ///
    /// 替代历史上依赖隐式 Drop 的模式；默认空实现。
    fn shutdown(&mut self) {}
}

// ---------------------------------------------------------------------------
// Legacy adapter
// ---------------------------------------------------------------------------

/// 将旧 [`OuterApp`] 包装为 [`AppPlugin`] 的兼容适配器。
///
/// 在兼容窗口内使用：将已有 demo 的 `OuterApp` 实现无修改地接入新的
/// `FrameRuntime` 阶段调度。所有 demo 迁移到原生 `AppPlugin` 后可移除。
#[allow(deprecated)]
pub struct LegacyOuterAppAdapter {
    inner: Box<dyn OuterApp>,
}

#[allow(deprecated)]
impl LegacyOuterAppAdapter {
    pub fn new(outer_app: Box<dyn OuterApp>) -> Self {
        Self { inner: outer_app }
    }
}

#[allow(deprecated)]
impl AppPlugin for LegacyOuterAppAdapter {
    fn init(&mut self, renderer: &mut Renderer, camera: &mut Camera) {
        self.inner.init(renderer, camera);
    }

    fn update(&mut self, renderer: &mut Renderer) {
        self.inner.update(renderer);
    }

    fn build_ui(&mut self, ui: &imgui::Ui) {
        self.inner.draw_ui(ui);
    }

    fn render(&self, renderer: &Renderer, gui_draw_data: &imgui::DrawData, timeline: &GfxSemaphore) {
        self.inner.draw(renderer, gui_draw_data, timeline);
    }

    fn on_resize(&mut self, renderer: &mut Renderer) {
        self.inner.on_window_resized(renderer);
    }
}
