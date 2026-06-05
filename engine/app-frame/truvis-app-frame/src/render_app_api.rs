//! 渲染线程和 `RenderAppShell` 帧骨架使用的 App 契约。
//!
//! 本模块把外部 render loop 看到的 object-safe [`RenderApp`] 与具体 App
//! 实现的 [`RenderAppHooks`] 分开：平台层只需要持有 `Box<dyn RenderApp>`，
//! frame runtime 则负责把 `RenderRuntime` 的生命周期阶段裁剪成 hook ctx。

use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use truvis_render_foundation::render_view::RenderView;
use truvis_render_runtime::render_runtime::{
    RenderRuntimeInitCtx, RenderRuntimeRayCastCtx, RenderRuntimeRenderCtx, RenderRuntimeResizeCtx,
    RenderRuntimeShutdownCtx, RenderRuntimeUpdateCtx,
};

use crate::input_event::InputEvent;
use crate::plugin_api::Plugin;

/// 由 render loop 驱动的 object-safe 外部契约。
///
/// 该 trait 是平台层和 frame runtime 之间的最窄边界。render loop 不知道具体
/// App 类型、Plugin 列表或 `RenderRuntime` 实现，只按窗口生命周期、输入事件、
/// resize、帧驱动和 shutdown 这些阶段调用这里的方法。
pub trait RenderApp {
    /// 在平台窗口创建完成后绑定渲染运行时和 App 状态。
    ///
    /// render loop 只应调用一次，并且必须早于 [`RenderApp::run_frame`]、
    /// [`RenderApp::recreate_swapchain_if_needed`] 和 [`RenderApp::shutdown`]。
    /// raw handles 来自平台层，通常由实现方创建 surface/swapchain，再进入具体
    /// App 与 Plugin 的初始化阶段。
    fn init_after_window(
        &mut self,
        raw_display: RawDisplayHandle,
        raw_window: RawWindowHandle,
        scale_factor: f64,
        window_size: [u32; 2],
    );

    /// 执行一帧完整渲染流程。
    ///
    /// `RenderAppShell` 的实现顺序是 begin frame、派发输入、App update、
    /// Plugin update、runtime prepare、App after_prepare、App render、present、end frame。
    /// 其他实现也应保持同样的阶段边界，避免 App/Plugin 在错误阶段访问 GPU
    /// 或帧状态。
    fn run_frame(&mut self);

    /// 将平台输入事件送入渲染线程侧队列。
    ///
    /// 该方法只表达事件传递契约，不要求立即处理事件。标准实现会在下一次
    /// [`RenderApp::run_frame`] 的 input 阶段批量交给 [`RenderAppHooks::on_input`]。
    fn push_input_event(&mut self, event: InputEvent);

    /// 在最新物理窗口尺寸变化后尝试重建 swapchain 相关资源。
    ///
    /// 调用方已经过滤掉零尺寸窗口；实现方仍可以在 runtime 判断无需重建时直接
    /// 返回。只有实际重建时才应触发 App resize hook 和 Plugin resize 生命周期。
    fn recreate_swapchain_if_needed(&mut self, new_size: [u32; 2]);

    /// 查询当前是否应该渲染下一帧。
    ///
    /// render loop 用它做帧节流或等待策略。返回 `false` 时，平台层可以短暂 park
    /// 渲染线程，而不改变 App 或 GPU 资源生命周期。
    fn time_to_render(&self) -> bool;

    /// 查询 runtime 是否已经记录了 swapchain 重建请求。
    ///
    /// 标准实现会在 acquire/present 返回 out-of-date 或 suboptimal 后置位，让 render loop
    /// 即使窗口尺寸没有再次变化，也能在安全点重建 swapchain。
    fn has_pending_swapchain_recreate(&self) -> bool {
        false
    }

    /// 退出渲染线程前释放 App/Plugin 持有的 GPU 资源。
    ///
    /// 标准实现会先等待 GPU idle，再调用 App shutdown hook 和 Plugin shutdown，
    /// 最后销毁 `RenderRuntime` root owner。App/Plugin 的显式释放必须发生在
    /// runtime destroy 之前。
    fn shutdown(&mut self);
}

/// `RenderAppShell` 传给 app hooks 的窗口绑定初始化上下文。
pub struct RenderAppInitCtx<'a> {
    /// 初始化阶段的 runtime 能力集合，由 shell 从 `RenderRuntimeInitCtx` 直接转交。
    pub runtime: RenderRuntimeInitCtx<'a>,
    /// 平台窗口的缩放因子，用于 GUI 或输入系统建立 display scale。
    pub scale_factor: f64,
    /// 初始化时的物理窗口尺寸，单位为像素。
    pub window_size: [u32; 2],
}

/// swapchain 资源变化时，`RenderAppShell` 传给 app hooks 的 resize 上下文。
pub struct RenderAppResizeCtx<'a> {
    /// resize 阶段的 runtime 能力集合，只在本次 resize 回调内有效。
    pub runtime: RenderRuntimeResizeCtx<'a>,
    /// 重建后的物理窗口尺寸，单位为像素。
    pub window_size: [u32; 2],
}

/// `RenderAppShell` 传给 app hooks 的 shutdown 上下文。
pub struct RenderAppShutdownCtx<'a> {
    /// shutdown 阶段的 runtime 能力集合，用于释放 App 自己持有的 GPU 资源。
    pub runtime: RenderRuntimeShutdownCtx<'a>,
}

/// 由 `RenderAppShell` 驱动的具体 app hook 契约。
///
/// 具体 App 持有 GUI、camera/input state、overlay 和 render pipeline plugin。
/// shell 持有 `RenderRuntime` 与输入队列，并通过这些 hook 交出生命周期和帧阶段
/// 控制点。App 负责定义输入消费策略、render graph 构建顺序以及特有 Plugin
/// 能力的调用位置。
pub trait RenderAppHooks {
    /// 初始化具体 App 自己的状态。
    ///
    /// 该 hook 发生在 runtime 完成窗口绑定之后、标准 Plugin `init` 之前。
    /// 适合建立 App 级状态，或准备后续 Plugin 初始化需要的 CPU/GPU 资源。
    fn init(&mut self, ctx: &mut RenderAppInitCtx<'_>);

    /// 按 app 定义的稳定顺序访问标准生命周期 plugin。
    ///
    /// `RenderAppShell` 使用该顺序批量调用 `Plugin::init`、`Plugin::update`
    /// 和 `Plugin::on_resize`。GUI UI 构建和 RenderGraph pass 贡献等特有能力
    /// 仍由具体 app 通过具体 plugin 类型显式调用。
    fn visit_plugins_mut(&mut self, _visit: &mut dyn FnMut(&mut dyn Plugin)) {}

    /// 按 app 定义的 shutdown 顺序访问标准生命周期 plugin。
    ///
    /// 默认实现沿用正向顺序；持有依赖关系的 App 应覆盖为反向顺序，保证后创建或
    /// 依赖上游资源的 Plugin 先释放。`RenderAppShell` 只在 shutdown 阶段使用它。
    fn visit_plugins_mut_rev(&mut self, visit: &mut dyn FnMut(&mut dyn Plugin)) {
        self.visit_plugins_mut(visit);
    }

    /// 处理本帧开始前累积的平台输入事件。
    ///
    /// 输入消费策略属于 App 级职责，例如先让 GUI Plugin 判断是否消费事件，再把
    /// 未消费事件交给相机或 gameplay input state。标准 Plugin 的 `on_input` 不由
    /// shell 自动批量调用。
    fn on_input(&mut self, events: &[InputEvent]);

    /// 更新 App 自己的 CPU 状态。
    ///
    /// 该 hook 发生在 runtime update phase 中，早于标准 Plugin update 和 runtime
    /// prepare。适合更新相机、overlay、UI frame state、`RenderOptions` 或 App 自有配置。
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
    /// `RenderAppShell` 会先调用此 hook，再按 [`RenderAppHooks::visit_plugins_mut_rev`]
    /// 通知标准 Plugin shutdown。实现中不要依赖 runtime destroy 之后仍可访问 GPU
    /// root owner。
    fn shutdown(&mut self, _ctx: &mut RenderAppShutdownCtx<'_>) {}
}
