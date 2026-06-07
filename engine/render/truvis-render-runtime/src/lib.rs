//! 被 `RenderAppShell` 驱动的渲染运行时集成层。
//!
//! 本 crate 持有 `Gfx` root owner、CPU `World`、GPU resource/binding/timing owner 和 runtime 私有
//! `GpuScene`，并通过阶段化的 typed Ctx 暴露初始化、更新、渲染、resize 与 shutdown 能力。
//! 它只负责资源所有权、资产到 GPU 的桥接、swapchain/present、command/sync 与 prepare
//! 阶段的数据上传；具体 app、plugin、GUI 适配和 render graph 编排由上层 crate 决定。

/// 窗口 surface、swapchain image/view 与 present 同步对象的所有权边界。
pub mod present;

/// AssetHub mesh 事件到 vertex/index buffer 与 BLAS 的渲染侧管理器。
mod asset_mesh_manager;
/// AssetHub 纹理事件到 GPU image/view/bindless 绑定的渲染侧管理器。
mod asset_texture_manager;

pub mod bindless_manager;
pub mod cmd_allocator;
pub mod descriptor_bindings;
pub mod dlss_sr;
mod environment_binding;
pub mod frame_state;
mod frame_timer;
pub mod frame_timing;
pub mod gfx_resource_manager;
pub mod global_descriptor_sets;
mod instance_bridge;
mod material_bridge;
mod material_manager;
pub mod per_frame_gpu_data;
/// prepare 后供 App 同步查询可见表面命中的 raycast API。
pub mod ray_cast;
pub mod render_options;
/// `RenderRuntime` 及其阶段化上下文，是上层 runtime 直接驱动的主入口。
pub mod render_runtime;
pub mod render_runtime_ctx;
mod render_scene;
mod runtime_defaults;
pub mod sampler_manager;
mod scene_bridge;
pub mod shader_binding_system;
mod sky_bridge;
pub mod stage_buffer_manager;
mod texture_resolver;
pub mod view_accum;
