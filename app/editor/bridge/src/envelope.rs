use crate::protocol::{ClientId, EditorNotification, EditorRequest, EditorResponse, RequestId};

/// Server 写入 Render request inbox 的单条请求。
///
/// `client_id` 只用于把结果路由回 WebSocket connection，领域请求 payload 在 Server
/// 与 App 之间保持不变。
#[derive(Clone, Debug)]
pub struct EditorRequestEnvelope {
    pub client_id: ClientId,
    pub request_id: RequestId,
    pub request: EditorRequest,
}

/// Render 写入 Server response outbox 的单条响应。
#[derive(Clone, Debug)]
pub struct EditorResponseEnvelope {
    pub client_id: ClientId,
    pub request_id: RequestId,
    pub response: EditorResponse,
}

/// Editor notification 的传递目标。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorNotificationTarget {
    Broadcast,
    Client(ClientId),
}

/// Render 写入 Server notification outbox 的单条 best-effort 通知。
#[derive(Clone, Debug)]
pub struct EditorNotificationEnvelope {
    pub target: EditorNotificationTarget,
    pub notification: EditorNotification,
}
