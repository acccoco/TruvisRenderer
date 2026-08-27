//! Truvis Web 编辑器的协议与跨线程通信边界。
//!
//! 本 crate 只拥有 editor DTO、transport envelope 和有界 channel endpoint。它不依赖
//! `truvis-world`、render runtime 或任何 GPU 类型，也不缓存 selection、scene 或 material 状态。

mod app_endpoint;
mod desktop_endpoint;
mod envelope;
pub mod protocol;

use tokio::sync::mpsc;

pub use app_endpoint::AppEndpoint;
pub use desktop_endpoint::DesktopEndpoint;
pub use envelope::EditorRequestEnvelope;

/// EditorBridge 两条有界队列的容量配置。
///
/// request 与 notification 分离后，可以分别表达拒绝新请求和 best-effort 通知丢弃。
/// response 由每个请求自带的 oneshot 返回，不构成共享队列或场景状态缓存。
#[derive(Clone, Copy, Debug)]
pub struct EditorBridgeConfig {
    pub request_capacity: usize,
    pub notification_capacity: usize,
}

impl Default for EditorBridgeConfig {
    fn default() -> Self {
        Self {
            request_capacity: 256,
            notification_capacity: 64,
        }
    }
}

/// 创建方向受限的 Desktop / App endpoint。
///
/// 两条队列全部有界；Render 侧只使用 `try_recv` / `try_send`，每个 request 通过独立
/// oneshot 返回 response。双方不会共享 `Mutex` 或场景对象。
pub fn create_editor_bridge(config: EditorBridgeConfig) -> (DesktopEndpoint, AppEndpoint) {
    let (request_sender, request_receiver) = mpsc::channel(config.request_capacity);
    let (notification_sender, notification_receiver) = mpsc::channel(config.notification_capacity);

    (
        DesktopEndpoint::new(request_sender, notification_receiver),
        AppEndpoint::new(request_receiver, notification_sender),
    )
}
