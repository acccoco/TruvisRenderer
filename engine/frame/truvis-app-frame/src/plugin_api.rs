//! 统一的 Plugin trait 与面向 plugin 的阶段上下文。
//!
//! Plugin 是由具体 App 持有的能力单元。frame runtime 只批量驱动标准生命周期，
//! 不负责发现 Plugin 的特有能力，也不通过注册表或 downcast 调用 render graph
//! pass 贡献、GUI 构建等 App 语义操作。

use truvis_gfx::commands::semaphore::GfxSemaphore;
use truvis_gfx::gfx::{GfxDeviceCtx, GfxDeviceInfoCtx, GfxImmediateCtx, GfxQueueCtx, GfxResourceCtx, GfxSurfaceCtx};
use truvis_gfx::swapchain::swapchain::GfxSwapchainImageInfo;
use truvis_render_foundation::cmd_allocator::CmdAllocator;
use truvis_render_foundation::gpu_store::GpuStore;
use truvis_render_foundation::pipeline_settings::{FrameSettings, PipelineSettings};
use truvis_render_foundation::render_scene_view::RenderSceneView;
use truvis_render_runtime::present::render_present::PresentView;
use truvis_world::World;

use crate::input_event::InputEvent;

/// 由 app 持有的 plugin 的一次性初始化上下文。
///
/// 该上下文来自 runtime 的初始化阶段，包含创建长期 GPU 资源、上传初始数据、
/// 注册 gpu-store 资源所需的能力。字段借用只在 `Plugin::init` 调用期间有效，
/// Plugin 不应长期保存 typed `Gfx` ctx 或 runtime 内部引用。
pub struct PluginInitCtx<'a> {
    /// 设备级 Vulkan 操作能力，用于创建依赖 logical device 的对象。
    pub device_ctx: GfxDeviceCtx<'a>,
    /// 资源管理能力，用于创建 buffer/image/view 等 GPU 资源。
    pub resource_ctx: GfxResourceCtx<'a>,
    /// 队列提交相关能力，用于初始化阶段的上传或一次性命令提交。
    pub queue_ctx: GfxQueueCtx<'a>,
    /// 物理设备与 feature 查询信息，用于按设备能力选择资源或 pipeline 路径。
    pub device_info_ctx: GfxDeviceInfoCtx<'a>,
    /// immediate 提交能力，用于初始化期的同步上传和短生命周期命令。
    pub immediate_ctx: GfxImmediateCtx<'a>,
    /// surface 相关能力，用于依赖窗口 surface 的初始化资源。
    pub surface_ctx: GfxSurfaceCtx<'a>,
    /// CPU 语义世界，可在初始化时注册或读取 App/scene 侧状态。
    pub world: &'a mut World,
    /// GPU-facing 状态仓库，可注册 Plugin 持有或依赖的 GPU 资源和全局渲染状态。
    pub gpu_store: &'a mut GpuStore,
    /// 命令分配器，供初始化阶段创建一次性或持久命令资源。
    pub cmd_allocator: &'a mut CmdAllocator,
    /// 当前 swapchain image 信息，供创建尺寸或格式相关资源。
    pub swapchain_image_info: GfxSwapchainImageInfo,
    /// present 资源只读视图，供 Plugin 查询 swapchain/present 相关句柄。
    pub render_present: PresentView<'a>,
}

/// 由 app 持有的 plugin 的 CPU 更新上下文。
///
/// 该上下文不提供 command recording 能力，主要用于更新 CPU 状态、读取帧设置、
/// 调整 pipeline settings，或把 `World` 中的语义状态推进到下一帧。
pub struct PluginUpdateCtx<'a> {
    /// CPU 语义世界，Plugin 可在 update 阶段修改其中的运行时状态。
    pub world: &'a mut World,
    /// 可变 pipeline 设置，供 UI 或控制类 Plugin 调整渲染管线参数。
    pub pipeline_settings: &'a mut PipelineSettings,
    /// 当前帧只读设置快照，供 Plugin 按帧状态做更新决策。
    pub frame_settings: &'a FrameSettings,
    /// 与上一帧之间的时间间隔，单位为秒。
    pub delta_time_s: f32,
}

/// 由 app 持有的 plugin 的渲染上下文。
///
/// 该上下文面向渲染录制和 render graph pass 贡献。它提供只读 `GpuStore`、
/// scene view、present 和队列同步相关能力，但不包含 App 级 GUI draw data；
/// GUI draw data 刻意保留在具体 GUI plugin 内部。
pub struct PluginRenderCtx<'a> {
    /// 设备级 Vulkan 操作能力，用于录制或绑定依赖 device 的对象。
    pub device_ctx: GfxDeviceCtx<'a>,
    /// 资源访问能力，用于引用本帧渲染所需的 GPU 资源。
    pub resource_ctx: GfxResourceCtx<'a>,
    /// 队列能力，用于 render graph 或 pass 录制需要的 queue 信息。
    pub queue_ctx: GfxQueueCtx<'a>,
    /// 设备能力只读信息，供 pass 根据 feature 选择渲染路径。
    pub device_info_ctx: GfxDeviceInfoCtx<'a>,
    /// 只读 GPU-facing 状态仓库，暴露全局 GPU 资源、manager 和帧状态。
    pub gpu_store: &'a GpuStore,
    /// runtime 准备好的场景只读视图，供 pass 访问 scene buffer、TLAS 和 draw 数据。
    pub render_scene: &'a dyn RenderSceneView,
    /// present 资源只读视图，供 pass 导入 swapchain 或 present target。
    pub render_present: PresentView<'a>,
    /// 帧 timeline semaphore，供需要显式同步信息的渲染路径引用。
    pub timeline: &'a GfxSemaphore,
}

/// 持有 swapchain 尺寸资源的 plugin 使用的 resize 上下文。
///
/// 该上下文只在 runtime 确认 swapchain 或窗口尺寸相关资源发生重建后出现。
/// Plugin 应在这里释放或重建自己持有的窗口尺寸相关资源；manager-owned image/view
/// 必须继续通过 `GpuStore` 中的 manager 释放。
pub struct PluginResizeCtx<'a> {
    /// 设备级 Vulkan 操作能力，用于重建依赖 logical device 的对象。
    pub device_ctx: GfxDeviceCtx<'a>,
    /// 资源管理能力，用于创建或释放尺寸相关 GPU 资源。
    pub resource_ctx: GfxResourceCtx<'a>,
    /// immediate 提交能力，用于 resize 期间需要同步完成的资源迁移。
    pub immediate_ctx: GfxImmediateCtx<'a>,
    /// surface 相关能力，用于依赖窗口 surface 的重建流程。
    pub surface_ctx: GfxSurfaceCtx<'a>,
    /// 可变 GPU-facing 状态仓库，供 Plugin 更新或释放其注册的 GPU 资源。
    pub gpu_store: &'a mut GpuStore,
    /// 新的 present 资源只读视图，供 Plugin 查询重建后的 swapchain 状态。
    pub render_present: PresentView<'a>,
}

/// 由 app 持有的 plugin 的 GPU shutdown 上下文。
///
/// shutdown 阶段发生在 runtime root owner 销毁之前。Plugin 必须在这里显式释放
/// 自己持有的 Vulkan/VMA/WSI 资源；之后字段 `Drop` 不应再调用底层销毁 API。
pub struct PluginShutdownCtx<'a> {
    /// 设备级 Vulkan 操作能力，用于销毁依赖 logical device 的对象。
    pub device_ctx: GfxDeviceCtx<'a>,
    /// 资源管理能力，用于释放 Plugin-owned buffer/image/view 等资源。
    pub resource_ctx: GfxResourceCtx<'a>,
    /// 队列能力，供释放流程中需要 queue 语义的资源使用。
    pub queue_ctx: GfxQueueCtx<'a>,
    /// immediate 提交能力，用于 shutdown 期间需要同步完成的清理工作。
    pub immediate_ctx: GfxImmediateCtx<'a>,
    /// surface 相关能力，用于释放依赖窗口 surface 的资源。
    pub surface_ctx: GfxSurfaceCtx<'a>,
    /// 可变 GPU-facing 状态仓库，供 Plugin 通过 manager 释放已注册的 GPU 资源。
    pub gpu_store: &'a mut GpuStore,
    /// 命令分配器，供 Plugin 释放自己持有或登记的命令资源。
    pub cmd_allocator: &'a mut CmdAllocator,
}

/// 可复用、由 app 持有的能力单元标准生命周期。
///
/// 该 trait 只覆盖 shell 能统一批量调用的 init / input / update / resize /
/// shutdown 生命周期。`ui()`、`begin_frame()`、`contribute_passes()` 或
/// `contribute_compute_passes()` 等特有能力保留在具体 Plugin 类型上，这样 App
/// 可以用显式字段组合能力，而无需 downcast、注册表或消息总线。
pub trait Plugin {
    /// 初始化 Plugin-owned 长期资源。
    ///
    /// 默认实现为空，适用于纯 CPU 或无需初始化资源的 Plugin。
    fn init(&mut self, _ctx: &mut PluginInitCtx) {}

    /// 处理单个输入事件，并返回该事件是否已被消费。
    ///
    /// shell 不会自动批量调用此方法；具体 App 可在自己的输入策略中显式调用。
    /// 返回 `true` 通常表示后续相机或业务输入不应再处理该事件。
    fn on_input(&mut self, _event: &InputEvent) -> bool {
        false
    }

    /// 更新 Plugin 的 CPU 状态。
    ///
    /// 该方法由 shell 在 App update 之后按 `visit_plugins_mut` 顺序调用。
    fn update(&mut self, _ctx: &mut PluginUpdateCtx) {}

    /// 响应 swapchain 或窗口尺寸相关资源重建。
    ///
    /// 该方法由 shell 在 App resize hook 之后按 `visit_plugins_mut` 顺序调用。
    fn on_resize(&mut self, _ctx: &mut PluginResizeCtx) {}

    /// 显式释放 Plugin-owned GPU 资源。
    ///
    /// 该方法由 shell 在 App shutdown hook 之后按 `visit_plugins_mut_rev` 顺序调用，
    /// 并且必须早于 runtime root owner 销毁。
    fn shutdown(&mut self, _ctx: &mut PluginShutdownCtx<'_>) {}
}
