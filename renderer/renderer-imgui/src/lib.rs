//! Renderer 层 ImGui 子系统、私有 Vulkan 后端和与具体渲染模式无关的诊断控件。

mod backend;
pub mod subsystem;
pub mod widgets;

pub use subsystem::ImGuiSubsystem;
pub use widgets::{DebugImageSelectorView, DebugInfoOverlay, FrameStatsOverlayData};
