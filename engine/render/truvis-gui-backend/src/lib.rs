//! ImGui Vulkan 渲染后端
//!
//! 本 crate 只提供底层 Vulkan 录制与 mesh buffer 类型。
//!
//! imgui `Context`、字体纹理注册、mesh 上传调度和 RenderGraph 适配由上层
//! `truvis_app_kit::gui_plugin::GuiPlugin` 持有，避免本 crate 反向依赖 frame
//! runtime 或 render graph。

pub mod gui_mesh;
pub mod gui_pass;
pub mod gui_vertex_layout;
