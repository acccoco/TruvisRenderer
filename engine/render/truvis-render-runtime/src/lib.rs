//! 被 `RenderAppShell` 驱动的渲染运行时集成层。
//!
//! 本 crate 持有 `Gfx` root owner、CPU `World`、GPU `GpuStore` 和 runtime 私有
//! `GpuScene`，并通过阶段化的 typed Ctx 暴露初始化、更新、渲染、resize 与 shutdown 能力。
//! 它只负责资源所有权、资产到 GPU 的桥接、swapchain/present、command/sync 与 prepare
//! 阶段的数据上传；具体 app、plugin、GUI 适配和 render graph 编排由上层 crate 决定。

/// 与窗口无关的 runtime 平台辅助类型，例如默认相机与帧计时器。
pub mod platform;
/// 窗口 surface、swapchain image/view 与 present 同步对象的所有权边界。
pub mod present;
/// 旧式渲染子系统扩展点；当前主流程优先通过 runtime 生命周期和上层 plugin 驱动。
pub mod subsystems;

/// AssetHub mesh 事件到 vertex/index buffer 与 BLAS 的渲染侧上传器。
mod asset_mesh_uploader;
/// AssetHub 纹理事件到 GPU image/view/bindless 绑定的渲染侧上传器。
mod asset_texture_uploader;
mod instance_bridge;
mod material_bridge;
mod material_manager;
/// prepare 后供 App 同步查询可见表面命中的 raycast API。
pub mod ray_cast;
/// `RenderRuntime` 及其阶段化上下文，是上层 runtime 直接驱动的主入口。
pub mod render_runtime;
mod render_scene;
mod scene_bridge;
mod texture_resolver;
