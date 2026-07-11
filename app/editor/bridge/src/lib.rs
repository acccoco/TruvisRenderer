//! Truvis Web 编辑器的协议与跨线程通信边界。
//!
//! 本 crate 只拥有 editor DTO、transport envelope 和有界 channel endpoint。它不依赖
//! `truvis-world`、render runtime 或任何 GPU 类型，也不缓存 selection、scene 或 material 状态。

mod app_endpoint;
mod envelope;
pub mod protocol;
mod server_endpoint;

use tokio::sync::mpsc;

pub use app_endpoint::AppEndpoint;
pub use envelope::{
    EditorNotificationEnvelope, EditorNotificationTarget, EditorRequestEnvelope, EditorResponseEnvelope,
};
pub use server_endpoint::ServerEndpoint;

/// EditorBridge 三条有界队列的容量配置。
///
/// request、response 和 notification 分离后，可以分别表达拒绝新请求、请求超时和
/// best-effort 通知丢弃。容量只限制短期通信数据，不构成场景状态缓存。
#[derive(Clone, Copy, Debug)]
pub struct EditorBridgeConfig {
    pub request_capacity: usize,
    pub response_capacity: usize,
    pub notification_capacity: usize,
}

impl Default for EditorBridgeConfig {
    fn default() -> Self {
        Self {
            request_capacity: 256,
            response_capacity: 256,
            notification_capacity: 64,
        }
    }
}

/// 创建方向受限的 Server / App endpoint。
///
/// 三条队列全部有界；Render 侧只使用 `try_recv` / `try_send`，Server 侧可以异步等待
/// response 和 notification。双方不会共享 `Mutex` 或场景对象。
pub fn create_editor_bridge(config: EditorBridgeConfig) -> (ServerEndpoint, AppEndpoint) {
    let (request_sender, request_receiver) = mpsc::channel(config.request_capacity);
    let (response_sender, response_receiver) = mpsc::channel(config.response_capacity);
    let (notification_sender, notification_receiver) = mpsc::channel(config.notification_capacity);

    (
        ServerEndpoint::new(request_sender, response_receiver, notification_receiver),
        AppEndpoint::new(request_receiver, response_sender, notification_sender),
    )
}
