//! Renderer 内部静态组合的子系统生命周期与窄化渲染上下文。
//!
//! 子系统始终由具体 `Renderer` 持有并显式编排，不经过注册表、动态分发或
//! `RenderLoop` 批量调度。该 trait 只约束需要显式管理 GPU 资源的生命周期；
//! 输入处理、UI 构建和 RenderGraph pass 贡献等具体能力继续留在各自类型上。

use truvis_gfx::commands::semaphore::GfxSemaphore;
use truvis_gfx::gfx::{GfxDeviceCtx, GfxDeviceInfoCtx, GfxQueueCtx, GfxResourceCtx};
use truvis_render_foundation::render_scene_view::RenderSceneView;
use truvis_render_runtime::present::swapchain_presenter::PresentView;
use truvis_render_runtime::render_runtime::{
    RenderRuntimeInitCtx, RenderRuntimeRenderCtx, RenderRuntimeResizeCtx, RenderRuntimeShutdownCtx,
};
use truvis_render_runtime::render_runtime_ctx::RenderPassRecordCtx;

/// 需要显式初始化和释放资源的 Renderer 内部子系统生命周期。
///
/// 生命周期顺序由拥有者 `Renderer` 决定。`init` 和 `shutdown` 必须成对实现，
/// `on_resize` 只供真正拥有窗口或渲染尺寸相关资源的类型按需覆盖。
pub trait SubsystemLifecycle {
    /// 创建子系统持有的长期 CPU/GPU 资源。
    fn init(&mut self, ctx: &mut RenderRuntimeInitCtx<'_>);

    /// 响应 swapchain 或渲染尺寸变化。
    fn on_resize(&mut self, _ctx: &mut RenderRuntimeResizeCtx<'_>) {}

    /// 在 runtime root owner 销毁之前显式释放子系统持有的 GPU 资源。
    fn shutdown(&mut self, ctx: &mut RenderRuntimeShutdownCtx<'_>);
}

/// Renderer 传给具体渲染子系统的只读录制上下文。
///
/// 该上下文刻意不包含 `world_submesh_raster` 等 Renderer 专属能力；需要这些能力的
/// 业务渲染器应由 Renderer 单独显式传入，避免把完整 runtime 权限扩散给所有子系统。
pub struct SubsystemRenderCtx<'a> {
    pub device_ctx: GfxDeviceCtx<'a>,
    pub resource_ctx: GfxResourceCtx<'a>,
    pub queue_ctx: GfxQueueCtx<'a>,
    pub device_info_ctx: GfxDeviceInfoCtx<'a>,
    pub record_ctx: RenderPassRecordCtx<'a>,
    pub render_scene: &'a dyn RenderSceneView,
    pub present: PresentView<'a>,
    pub timeline: &'a GfxSemaphore,
}

impl<'a> SubsystemRenderCtx<'a> {
    /// 从 Renderer 的 render 阶段上下文裁剪出子系统实际需要的只读能力。
    pub fn from_runtime(ctx: &RenderRuntimeRenderCtx<'a>) -> Self {
        Self {
            device_ctx: ctx.device_ctx,
            resource_ctx: ctx.resource_ctx,
            queue_ctx: ctx.queue_ctx,
            device_info_ctx: ctx.device_info_ctx,
            record_ctx: ctx.record_ctx,
            render_scene: ctx.render_scene,
            present: ctx.present,
            timeline: ctx.timeline,
        }
    }
}
