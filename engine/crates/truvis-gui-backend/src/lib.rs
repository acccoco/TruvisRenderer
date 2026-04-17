//! ImGui Vulkan 渲染后端
//!
//! ImGui 系统跨多个 crate 分为三层：
//!
//! | 层       | 位置              | 职责                                     |
//! |----------|-------------------|------------------------------------------|
//! | 平台层   | `truvis-app` (`GuiHost`)     | 管理 imgui Context、处理输入事件、字体加载 |
//! | 渲染层   | **本 crate** (`GuiBackend` + `GuiPass`) | 字体纹理上传、mesh 缓冲、Vulkan 命令录制 |
//! | 应用层   | `truvis-app` (`GuiRgPass` + `OuterApp::draw_ui`) | RenderGraph 适配、用户 UI 构建 |
//!
//! 本 crate 是纯 Vulkan 录制层，不依赖 render-graph。
//! RenderGraph 适配（`GuiRgPass`）由 `truvis-app` 负责，保持本 crate 与 render-graph 解耦。

pub mod gui_backend;
pub mod gui_mesh;
pub mod gui_pass;
pub mod gui_vertex_layout;
