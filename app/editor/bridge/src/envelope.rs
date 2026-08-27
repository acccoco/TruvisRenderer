use tokio::sync::oneshot;

use crate::protocol::{EditorRequest, EditorResponse};

/// Desktop 写入 Render request inbox 的单条请求。
///
/// 每个 Tauri invoke 自带独立 reply，因此不需要额外的 request/client 路由表。
#[derive(Debug)]
pub struct EditorRequestEnvelope {
    pub request: EditorRequest,
    pub reply: oneshot::Sender<EditorResponse>,
}
