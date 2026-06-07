use ash::vk;
use truvis_gfx::commands::semaphore::GfxSemaphore;
use truvis_gfx::gfx::{GfxDeviceCtx, GfxDeviceInfoCtx, GfxImmediateCtx, GfxQueueCtx, GfxResourceCtx, GfxSurfaceCtx};
use truvis_gfx::swapchain::swapchain::GfxSwapchainImageInfo;
use truvis_render_foundation::cmd_allocator::CmdAllocator;
use truvis_render_foundation::frame_state::FrameRenderState;
use truvis_render_foundation::frame_timing::FrameTiming;
use truvis_render_foundation::gfx_resource_manager::GfxResourceManager;
use truvis_render_foundation::render_options::RenderOptions;
use truvis_render_foundation::render_pass_record_ctx::RenderPassRecordCtx;
use truvis_render_foundation::render_scene_view::RenderSceneView;
use truvis_render_foundation::shader_binding_system::{ShaderBindingSystem, ShaderBindingView};
use truvis_render_foundation::view_accum::ViewAccumState;
use truvis_world::World;

use crate::instance_bridge::InstanceBridge;
use crate::present::swapchain_presenter::PresentView;
use crate::ray_cast::{RayCastRay, RayCastResult, RayCastService};

/// Update 阶段上下文，借用 CPU 端更新需要的 RenderRuntime 字段。
///
/// 在 app 执行 update 工作期间保持存活；drop 前 RenderRuntime 会保持借用锁定。
/// 这个阶段允许修改 `World` 与全局渲染选项，但还没有把 CPU 语义数据翻译到 GPU scene。
pub struct RenderRuntimeUpdateCtx<'a> {
    /// CPU 语义世界；update 阶段允许 app/plugin 修改 scene、asset 请求和运行时实例。
    pub world: &'a mut World,
    /// 可变全局渲染选项；修改后由 runtime 在 prepare/render 前统一同步派生状态。
    pub render_options: &'a mut RenderOptions,
    /// 当前帧渲染目标状态快照，已在 acquire 前与 swapchain 同步。
    pub frame_state: &'a FrameRenderState,
    /// 当前 main view 的累积状态，只读暴露给上层 UI 或调试逻辑。
    pub view_accum: &'a ViewAccumState,
    /// 当前 swapchain extent，便于 app 在 update 阶段同步相机纵横比。
    pub swapchain_extent: vk::Extent2D,
    /// 当前帧序号、FIF label 和时间快照。
    pub frame_timing: &'a FrameTiming,
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
    /// pass 录制需要的只读 GPU-facing 状态。
    pub record_ctx: RenderPassRecordCtx<'a>,
    /// runtime 私有 `GpuScene` 的只读视图；pass 不能访问 concrete scene owner。
    pub render_scene: &'a dyn RenderSceneView,
    /// 当前窗口 present 边界，只暴露 swapchain 信息和 RenderGraph 导入 helper。
    pub present: PresentView<'a>,
    /// runtime 全局 FIF timeline，用于 render graph signal 当前 frame id。
    pub timeline: &'a GfxSemaphore,
}

/// prepare 后、render 前的同步查询上下文。
///
/// 到达该阶段时 GPU scene 已完成 CPU->GPU 翻译并提交到 graphics queue。App 可以在这里
/// 发起同步 raycast，runtime 会阻塞等待 GPU trace 与 readback 完成，再返回 CPU handle 语义。
pub struct RenderRuntimeRayCastCtx<'a> {
    pub(crate) device_ctx: GfxDeviceCtx<'a>,
    pub(crate) resource_ctx: GfxResourceCtx<'a>,
    pub(crate) queue_ctx: GfxQueueCtx<'a>,
    pub(crate) frame_timing: &'a FrameTiming,
    pub(crate) shader_bindings: ShaderBindingView<'a>,
    pub(crate) render_scene: &'a dyn RenderSceneView,
    pub(crate) instance_bridge: &'a InstanceBridge,
    pub(crate) ray_cast_service: &'a mut RayCastService,
}

impl RenderRuntimeRayCastCtx<'_> {
    /// 同步执行一批 world-space raycast。
    ///
    /// 返回结果与输入 ray 顺序一致。该调用会提交 GPU ray tracing 命令并等待 fence，
    /// 因此应只用于 App 明确需要即时命中结果的交互路径。
    pub fn cast_sync(&mut self, rays: &[RayCastRay]) -> anyhow::Result<Vec<RayCastResult>> {
        self.ray_cast_service.cast_sync(
            self.resource_ctx,
            self.device_ctx,
            self.queue_ctx,
            self.frame_timing,
            self.shader_bindings,
            self.render_scene,
            self.instance_bridge,
            rays,
        )
    }
}

/// Init 阶段上下文，用于 window/surface 创建后的一次性设置。
///
/// 不包含 camera；camera 属于具体 app。
/// 这里暴露 `World`、GPU 资源/binding owner 和 `CmdAllocator` 的可变借用，供 app/plugin 创建长期 GPU 资源；
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
    /// manager-owned buffer/image/view 资源生命周期 owner。
    pub gfx_resource_manager: &'a mut GfxResourceManager,
    /// shader-visible descriptor、bindless 和 sampler owner。
    pub shader_binding_system: &'a mut ShaderBindingSystem,
    /// 当前帧序号、FIF label 和时间快照。
    pub frame_timing: &'a FrameTiming,
    /// 当前 main view / frame 的渲染目标状态。
    pub frame_state: &'a FrameRenderState,
    /// 命令分配器，供初始化阶段创建长期或一次性 command buffer。
    pub cmd_allocator: &'a mut CmdAllocator,
    /// 初始 swapchain image 信息，供上层创建窗口尺寸相关资源。
    pub swapchain_image_info: GfxSwapchainImageInfo,
    /// 初始化后可用的 present 边界只读引用。
    pub present: PresentView<'a>,
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
    /// manager-owned buffer/image/view 资源生命周期 owner。
    pub gfx_resource_manager: &'a mut GfxResourceManager,
    /// shader-visible descriptor、bindless 和 sampler owner。
    pub shader_binding_system: &'a mut ShaderBindingSystem,
    /// 当前帧序号、FIF label 和时间快照。
    pub frame_timing: &'a FrameTiming,
    /// resize 后的 main view / frame 渲染目标状态。
    pub frame_state: &'a FrameRenderState,
    /// 已重建完成的 present 边界只读引用。
    pub present: PresentView<'a>,
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
    /// 仍然存活的 manager-owned buffer/image/view 资源生命周期 owner。
    pub gfx_resource_manager: &'a mut GfxResourceManager,
    /// 仍然存活的 shader-visible descriptor、bindless 和 sampler owner。
    pub shader_binding_system: &'a mut ShaderBindingSystem,
    /// 当前帧序号、FIF label 和时间快照。
    pub frame_timing: &'a FrameTiming,
    /// shutdown 前最后的 main view / frame 渲染目标状态。
    pub frame_state: &'a FrameRenderState,
    /// 命令分配器仍然存活，供上层显式释放自己创建的 command 资源。
    pub cmd_allocator: &'a mut CmdAllocator,
}
