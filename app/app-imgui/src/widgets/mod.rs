//! 不依赖具体渲染子系统的通用 ImGui 诊断控件。

mod debug_image;
mod diagnostics;

pub use debug_image::DebugImageSelectorView;
pub use diagnostics::{DebugInfoOverlay, FrameStatsOverlayData};
