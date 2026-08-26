//! 渲染线程和 `RenderLoop` 帧骨架使用的 Renderer 契约。
//!
//! [`Renderer`] 表达具体产品或 sample 可以填充的渲染业务阶段；[`RenderLoop`](crate::RenderLoop)
//! 是唯一完整帧骨架，并负责把 `RenderRuntime` 的生命周期阶段裁剪成 hook ctx。

use truvis_render_foundation::render_view::RenderView;
use truvis_render_runtime::render_runtime::{
    RenderRuntimeInitCtx, RenderRuntimeRayCastCtx, RenderRuntimeRenderCtx, RenderRuntimeResizeCtx,
    RenderRuntimeShutdownCtx, RenderRuntimeUpdateCtx,
};

use crate::input_event::InputEvent;

/// `RenderLoop` 传给 Renderer hooks 的窗口绑定初始化上下文。
pub struct RendererInitCtx<'a> {
    /// 初始化阶段的 runtime 能力集合，由 RenderLoop 从 `RenderRuntimeInitCtx` 直接转交。
    pub runtime: RenderRuntimeInitCtx<'a>,
    /// 平台窗口的缩放因子，用于 GUI 或输入系统建立 display scale。
    pub scale_factor: f64,
    /// 初始化时的物理窗口尺寸，单位为像素。
    pub window_size: [u32; 2],
}

/// swapchain 资源变化时，`RenderLoop` 传给 Renderer hooks 的 resize 上下文。
pub struct RendererResizeCtx<'a> {
    /// resize 阶段的 runtime 能力集合，只在本次 resize 回调内有效。
    pub runtime: RenderRuntimeResizeCtx<'a>,
    /// 重建后的物理窗口尺寸，单位为像素。
    pub window_size: [u32; 2],
}

/// `RenderLoop` 传给 Renderer hooks 的 shutdown 上下文。
pub struct RendererShutdownCtx<'a> {
    /// shutdown 阶段的 runtime 能力集合，用于释放 Renderer 自己持有的 GPU 资源。
    pub runtime: RenderRuntimeShutdownCtx<'a>,
}

/// 由 `RenderLoop` 驱动的顶层渲染业务契约。
///
/// 具体 Renderer 持有 GUI、camera/input state、overlay 和具体渲染子系统。
/// `RenderLoop` 持有 `RenderRuntime` 与输入队列，并通过这些 hook 交出生命周期和帧阶段
/// 控制点。Renderer 负责定义输入消费策略、子系统生命周期、RenderGraph 构建顺序及
/// 各项具体能力的调用位置；`RenderLoop` 不感知 Renderer 内部静态组合。
pub trait Renderer {
    /// 初始化具体 Renderer 自己的状态和持有的具体子系统。
    ///
    /// 该 hook 发生在 runtime 完成窗口绑定之后。Renderer 自行决定 CPU 状态建立和
    /// 各子系统长期 GPU 资源创建的顺序。
    fn init(&mut self, ctx: &mut RendererInitCtx<'_>);

    /// 处理本帧开始前累积的平台输入事件。
    ///
    /// 输入消费策略属于 Renderer 级职责，例如先让 GUI 子系统判断是否消费事件，再把
    /// 未消费事件交给相机或 gameplay input state。
    fn on_input(&mut self, events: &[InputEvent]);

    /// 更新 Renderer 自己的 CPU 状态。
    ///
    /// 该 hook 发生在 runtime update phase 中，早于 runtime prepare。Renderer 在此
    /// 显式更新相机、overlay、UI frame state、`DlssOptions` 或其他自有状态。
    fn update(&mut self, ctx: &mut RenderRuntimeUpdateCtx);

    /// 在 runtime prepare 完成后、render graph 组图前执行 Renderer 同步查询。
    ///
    /// 该阶段 GPU scene/TLAS 已按当前 CPU world 与 camera 快照完成同步，适合调用
    /// `RenderRuntimeRayCastCtx::cast_sync` 做即时拾取。默认实现为空，避免未使用 raycast
    /// 的 Renderer 需要额外接入。
    fn after_prepare(&mut self, _ctx: &mut RenderRuntimeRayCastCtx<'_>) {}

    /// 构建并录制本帧 Renderer 语义下的渲染工作。
    ///
    /// Renderer 在这里创建 RenderGraph，并显式决定具体渲染子系统与 GUI pass 的加入顺序。
    /// 渲染能力保留在具体子系统类型上，不通过 `RenderLoop` 或通用生命周期 trait 派发。
    fn render(&mut self, ctx: &RenderRuntimeRenderCtx);

    /// 提供 runtime prepare 阶段使用的当前渲染视图。
    ///
    /// 相机所有权留在具体 Renderer 中，runtime 只在本帧 prepare 调用期间读取视图快照。
    fn render_view(&self) -> RenderView;

    /// 响应 swapchain 或窗口尺寸相关资源重建。
    ///
    /// 该 hook 只在 runtime 确认发生 resize 后调用。Renderer 在这里更新自身窗口尺寸
    /// 状态，并显式重建各子系统持有的 renderer-owned render targets。
    fn on_resize(&mut self, _ctx: &mut RendererResizeCtx<'_>) {}

    /// 按 Renderer 定义的顺序释放自己和各具体子系统持有的 GPU 资源。
    ///
    /// `RenderLoop` 在 GPU idle 之后、runtime root owner 销毁之前调用此 hook。
    /// Renderer 必须在该阶段完成全部显式资源释放，不应依赖后续字段 `Drop` 访问 Vulkan。
    fn shutdown(&mut self, _ctx: &mut RendererShutdownCtx<'_>) {}
}
