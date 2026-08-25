//! 渲染线程和 `RenderAppRunner` 帧骨架使用的 App 契约。
//!
//! [`RenderApp`] 表达具体 App 可以填充的业务阶段；[`RenderAppRunner`](crate::RenderAppRunner)
//! 是唯一完整帧骨架，并负责把 `RenderRuntime` 的生命周期阶段裁剪成 hook ctx。

use truvis_render_foundation::render_view::RenderView;
use truvis_render_runtime::render_runtime::{
    RenderRuntimeInitCtx, RenderRuntimeRayCastCtx, RenderRuntimeRenderCtx, RenderRuntimeResizeCtx,
    RenderRuntimeShutdownCtx, RenderRuntimeUpdateCtx,
};

use crate::input_event::InputEvent;
use crate::plugin_api::Plugin;

/// `RenderAppRunner` 传给 app hooks 的窗口绑定初始化上下文。
pub struct RenderAppInitCtx<'a> {
    /// 初始化阶段的 runtime 能力集合，由 Runner 从 `RenderRuntimeInitCtx` 直接转交。
    pub runtime: RenderRuntimeInitCtx<'a>,
    /// 平台窗口的缩放因子，用于 GUI 或输入系统建立 display scale。
    pub scale_factor: f64,
    /// 初始化时的物理窗口尺寸，单位为像素。
    pub window_size: [u32; 2],
}

/// swapchain 资源变化时，`RenderAppRunner` 传给 app hooks 的 resize 上下文。
pub struct RenderAppResizeCtx<'a> {
    /// resize 阶段的 runtime 能力集合，只在本次 resize 回调内有效。
    pub runtime: RenderRuntimeResizeCtx<'a>,
    /// 重建后的物理窗口尺寸，单位为像素。
    pub window_size: [u32; 2],
}

/// `RenderAppRunner` 传给 app hooks 的 shutdown 上下文。
pub struct RenderAppShutdownCtx<'a> {
    /// shutdown 阶段的 runtime 能力集合，用于释放 App 自己持有的 GPU 资源。
    pub runtime: RenderRuntimeShutdownCtx<'a>,
}

/// 由 `RenderAppRunner` 驱动的具体 App 契约。
///
/// 具体 App 持有 GUI、camera/input state、overlay 和 render pipeline plugin。
/// Runner 持有 `RenderRuntime` 与输入队列，并通过这些 hook 交出生命周期和帧阶段
/// 控制点。App 负责定义输入消费策略、render graph 构建顺序以及特有 Plugin
/// 能力的调用位置。
pub trait RenderApp {
    /// 初始化具体 App 自己的状态。
    ///
    /// 该 hook 发生在 runtime 完成窗口绑定之后、标准 Plugin `init` 之前。
    /// 适合建立 App 级状态，或准备后续 Plugin 初始化需要的 CPU/GPU 资源。
    fn init(&mut self, ctx: &mut RenderAppInitCtx<'_>);

    /// 按 app 定义的稳定顺序访问标准生命周期 plugin。
    ///
    /// `RenderAppRunner` 使用该顺序批量调用 `Plugin::init`、`Plugin::update`
    /// 和 `Plugin::on_resize`。GUI UI 构建和 RenderGraph pass 贡献等特有能力
    /// 仍由具体 app 通过具体 plugin 类型显式调用。
    fn visit_plugins_mut(&mut self, _visit: &mut dyn FnMut(&mut dyn Plugin)) {}

    /// 按 app 定义的 shutdown 顺序访问标准生命周期 plugin。
    ///
    /// 默认实现沿用正向顺序；持有依赖关系的 App 应覆盖为反向顺序，保证后创建或
    /// 依赖上游资源的 Plugin 先释放。`RenderAppRunner` 只在 shutdown 阶段使用它。
    fn visit_plugins_mut_rev(&mut self, visit: &mut dyn FnMut(&mut dyn Plugin)) {
        self.visit_plugins_mut(visit);
    }

    /// 处理本帧开始前累积的平台输入事件。
    ///
    /// 输入消费策略属于 App 级职责，例如先让 GUI Plugin 判断是否消费事件，再把
    /// 未消费事件交给相机或 gameplay input state。标准 Plugin 的 `on_input` 不由
    /// Runner 自动批量调用。
    fn on_input(&mut self, events: &[InputEvent]);

    /// 更新 App 自己的 CPU 状态。
    ///
    /// 该 hook 发生在 runtime update phase 中，早于标准 Plugin update 和 runtime
    /// prepare。适合更新相机、overlay、UI frame state、`DlssOptions` 或 App 自有配置。
    fn update(&mut self, ctx: &mut RenderRuntimeUpdateCtx);

    /// 在 runtime prepare 完成后、render graph 组图前执行 App 同步查询。
    ///
    /// 该阶段 GPU scene/TLAS 已按当前 CPU world 与 camera 快照完成同步，适合调用
    /// `RenderRuntimeRayCastCtx::cast_sync` 做即时拾取。默认实现为空，避免未使用 raycast
    /// 的 App 需要额外接入。
    fn after_prepare(&mut self, _ctx: &mut RenderRuntimeRayCastCtx<'_>) {}

    /// 构建并录制本帧 App 语义下的渲染工作。
    ///
    /// App 在这里创建 RenderGraph，并显式决定具体渲染 Plugin 与 GUI pass 的加入
    /// 顺序。通用 Plugin trait 不包含 pass 贡献能力，因此这里通常调用具体 Plugin
    /// 类型上的 render 方法。
    fn render(&mut self, ctx: &RenderRuntimeRenderCtx);

    /// 提供 runtime prepare 阶段使用的当前渲染视图。
    ///
    /// 相机所有权留在具体 App 中，runtime 只在本帧 prepare 调用期间读取视图快照。
    fn render_view(&self) -> RenderView;

    /// 响应 swapchain 或窗口尺寸相关资源重建。
    ///
    /// 该 hook 只在 runtime 确认发生 resize 后调用，早于标准 Plugin `on_resize`。
    /// App 可在这里更新自身持有的窗口尺寸状态或 App-owned render targets。
    fn on_resize(&mut self, _ctx: &mut RenderAppResizeCtx<'_>) {}

    /// 释放 App 自己持有的 GPU 资源。
    ///
    /// `RenderAppRunner` 会先调用此 hook，再按 [`RenderApp::visit_plugins_mut_rev`]
    /// 通知标准 Plugin shutdown。实现中不要依赖 runtime destroy 之后仍可访问 GPU
    /// root owner。
    fn shutdown(&mut self, _ctx: &mut RenderAppShutdownCtx<'_>) {}
}
