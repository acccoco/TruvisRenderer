//! Realtime/offline 渲染都可使用的设置、模式和 image target 契约。

mod debug_channel;
mod render_mode;
pub mod settings;
pub(crate) mod targets;

pub use debug_channel::PathTracingDebugChannel;
pub use render_mode::RenderMode;
pub use settings::{PathTracingCommonSettings, SdrToneMappingSettings, SkySamplingMode};
pub use targets::ImageTarget;
