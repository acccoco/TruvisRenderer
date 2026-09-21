//! App 共用的启动场景与参数；场景方法只由 RenderThread Client 调用。

mod scenes;
mod startup_options;

pub use scenes::{InitialScene, SceneInitializer};
pub use startup_options::StartupOptions;
