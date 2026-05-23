use ash::vk;
use truvis_gfx::commands::semaphore::GfxSemaphore;
use truvis_gfx::gfx::{GfxDeviceCtx, GfxDeviceInfoCtx, GfxImmediateCtx, GfxQueueCtx, GfxResourceCtx, GfxSurfaceCtx};
use truvis_gfx::swapchain::swapchain::GfxSwapchainImageInfo;
use truvis_render_foundation::cmd_allocator::CmdAllocator;
use truvis_render_foundation::gpu_store::GpuStore;
use truvis_render_foundation::pipeline_settings::{AccumData, FrameSettings, PipelineSettings};
use truvis_render_foundation::render_scene_view::RenderSceneView;
use truvis_world::World;

use crate::present::render_present::PresentView;

/// Update 阶段上下文，借用 CPU 端更新需要的 RenderRuntime 字段。
///
/// 在 app 执行 update 工作期间保持存活；drop 前 RenderRuntime 会保持借用锁定。
/// 这个阶段允许修改 `World` 与管线设置，但还没有把 CPU 语义数据翻译到 GPU scene。
pub struct RenderRuntimeUpdateCtx<'a> {
    /// CPU 语义世界；update 阶段允许 app/plugin 修改 scene、asset 请求和运行时实例。
    pub world: &'a mut World,
    /// 可变管线设置；修改会影响后续 prepare/render 阶段的 pass 行为。
    pub pipeline_settings: &'a mut PipelineSettings,
    /// 当前帧尺寸和格式快照，已在 acquire 前与 swapchain 同步。
    pub frame_settings: &'a FrameSettings,
    /// 累积渲染状态，只读暴露给上层 UI 或调试逻辑。
    pub accum_data: &'a AccumData,
    /// 当前 swapchain extent，便于 app 在 update 阶段同步相机纵横比。
    pub swapchain_extent: vk::Extent2D,
    /// `begin_frame` 计算出的上一帧 delta time，单位秒。
    pub delta_time_s: f32,
}

/// Render 阶段上下文，对 GPU 命令录制需要的 RenderRuntime 状态进行只读共享借用。
///
/// 到达这个阶段时 `prepare` 已经完成 per-frame descriptor、material buffer、scene buffer、
/// TLAS 和 raster draw cache 的更新；pass 只能读取这些结果并录制命令。
pub struct RenderRuntimeRenderCtx<'a> {
    /// Vulkan device 能力，只用于命令录制和对象访问，不转移所有权。
    pub device_ctx: GfxDeviceCtx<'a>,
    /// GPU 资源分配/释放上下文；render 阶段通常只应使用已有资源，避免临时 owner 泄漏。
    pub resource_ctx: GfxResourceCtx<'a>,
    /// 队列上下文，供 render graph submit 使用。
    pub queue_ctx: GfxQueueCtx<'a>,
    /// 设备能力查询上下文，供 pass 根据硬件限制选择路径。
    pub device_info_ctx: GfxDeviceInfoCtx<'a>,
    /// GPU 侧 frame state、descriptor、manager-owned resources 和 per-frame buffer。
    pub gpu_store: &'a GpuStore,
    /// runtime 私有 `GpuScene` 的只读视图；pass 不能访问 concrete scene owner。
    pub render_scene: &'a dyn RenderSceneView,
    /// 当前窗口 present target 与同步对象。
    pub render_present: PresentView<'a>,
    /// runtime 全局 FIF timeline，用于 render graph signal 当前 frame id。
    pub timeline: &'a GfxSemaphore,
}

/// Init 阶段上下文，用于 window/surface 创建后的一次性设置。
///
/// 不包含 camera；camera 属于具体 app。
/// 这里暴露 `World`、`GpuStore` 和 `CmdAllocator` 的可变借用，供 app/plugin 创建长期 GPU 资源；
/// 初始化完成后这些能力会重新收敛回 runtime 的阶段化生命周期。
pub struct RenderRuntimeInitCtx<'a> {
    /// 初始化长期 GPU 资源所需的 device 上下文。
    pub device_ctx: GfxDeviceCtx<'a>,
    /// 初始化长期 GPU 资源所需的资源上下文。
    pub resource_ctx: GfxResourceCtx<'a>,
    /// 初始化阶段可用的队列上下文。
    pub queue_ctx: GfxQueueCtx<'a>,
    /// 初始化阶段可用的设备能力查询上下文。
    pub device_info_ctx: GfxDeviceInfoCtx<'a>,
    /// 一次性上传/初始化资源使用的 immediate 上下文。
    pub immediate_ctx: GfxImmediateCtx<'a>,
    /// surface/swapchain 相关操作所需上下文。
    pub surface_ctx: GfxSurfaceCtx<'a>,
    /// CPU 语义世界，供 app/plugin 注册初始 scene、asset 和实例。
    pub world: &'a mut World,
    /// GPU frame state，供 app/plugin 创建长期 descriptor、buffer、pipeline 依赖。
    pub gpu_store: &'a mut GpuStore,
    /// 命令分配器，供初始化阶段创建长期或一次性 command buffer。
    pub cmd_allocator: &'a mut CmdAllocator,
    /// 初始 swapchain image 信息，供上层创建窗口尺寸相关资源。
    pub swapchain_image_info: GfxSwapchainImageInfo,
    /// 初始化后可用的 present owner 只读引用。
    pub render_present: PresentView<'a>,
}

/// Swapchain resize 上下文，仅在 swapchain 实际重建时产生。
///
/// 上层只在收到 `Some(ctx)` 时重建窗口尺寸相关资源；连续 resize 事件会在 present 层合并。
pub struct RenderRuntimeResizeCtx<'a> {
    /// resize 后重建上层 GPU 资源需要的 device 上下文。
    pub device_ctx: GfxDeviceCtx<'a>,
    /// resize 后重建上层 GPU 资源需要的资源上下文。
    pub resource_ctx: GfxResourceCtx<'a>,
    /// 需要立即上传 resize 相关资源时使用。
    pub immediate_ctx: GfxImmediateCtx<'a>,
    /// resize 路径访问 surface/swapchain 所需上下文。
    pub surface_ctx: GfxSurfaceCtx<'a>,
    /// resize 后的 GPU frame state，可用于重建窗口尺寸相关资源。
    pub gpu_store: &'a mut GpuStore,
    /// 已重建完成的 present owner。
    pub render_present: PresentView<'a>,
}

/// Shutdown 阶段上下文，保证 app/plugin 可在 runtime 与 Gfx 存活时释放 GPU 资源。
///
/// `RenderAppShell` 会在 runtime 自身销毁前把这个上下文交给 app/plugin，确保 plugin-owned
/// pipeline、buffer、descriptor 等资源仍能通过 typed Ctx 显式释放。
pub struct RenderRuntimeShutdownCtx<'a> {
    /// 释放 plugin/app-owned GPU 对象所需 device 上下文。
    pub device_ctx: GfxDeviceCtx<'a>,
    /// 释放 plugin/app-owned GPU 对象所需资源上下文。
    pub resource_ctx: GfxResourceCtx<'a>,
    /// 某些上层资源需要显式队列上下文完成 shutdown。
    pub queue_ctx: GfxQueueCtx<'a>,
    /// 释放前需要做最后一次 immediate 操作时使用。
    pub immediate_ctx: GfxImmediateCtx<'a>,
    /// surface 相关上层资源释放时使用。
    pub surface_ctx: GfxSurfaceCtx<'a>,
    /// 仍然存活的 GPU frame state；shutdown 完成后由 runtime destroy 接管。
    pub gpu_store: &'a mut GpuStore,
    /// 命令分配器仍然存活，供上层显式释放自己创建的 command 资源。
    pub cmd_allocator: &'a mut CmdAllocator,
}
