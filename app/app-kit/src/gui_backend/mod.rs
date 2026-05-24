//! App 层私有 ImGui Vulkan 后端。
//!
//! 这里只保存 `GuiPlugin` 的底层 GPU 实现细节：GUI mesh buffer、
//! imgui draw data 的 Vulkan 命令录制与 vertex layout。imgui context、
//! 输入转发、字体资源注册和 RenderGraph 适配仍由上层 `GuiPlugin` 持有。

pub(super) mod gui_mesh;
pub(super) mod gui_pass;
pub(super) mod gui_vertex_layout;
