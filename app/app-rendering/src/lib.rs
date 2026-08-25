//! 独立于 ImGui 的 realtime/offline 渲染子系统及长期 GPU 资源。

pub mod offline;
pub mod realtime;
pub mod shared;

pub use offline::{OfflineRenderSettings, OfflineRenderSubsystem};
pub use realtime::{RealtimeRenderSettings, RealtimeRenderSubsystem};
pub use shared::{ImageTarget, PathTracingCommonSettings, PathTracingDebugChannel, RenderMode, SkySamplingMode};
