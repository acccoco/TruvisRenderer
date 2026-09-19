use tokio::sync::mpsc::{Receiver, Sender};

use crate::EditorRequestEnvelope;
use crate::protocol::EditorNotification;

/// Frontend adapter 独占的跨线程 endpoint。
///
/// request sender 供 frontend command clone 后非阻塞提交请求；notification receiver 由
/// frontend 侧单个 async dispatcher 独占。该类型不保存 scene、selection 或 material 状态，
/// 也不依赖 Tauri、WebView 或其他具体传输实现。
pub struct FrontendEndpoint {
    request_sender: Sender<EditorRequestEnvelope>,
    notification_receiver: Receiver<EditorNotification>,
}

impl FrontendEndpoint {
    pub(crate) fn new(
        request_sender: Sender<EditorRequestEnvelope>,
        notification_receiver: Receiver<EditorNotification>,
    ) -> Self {
        Self {
            request_sender,
            notification_receiver,
        }
    }

    /// 把 endpoint 拆成 Tauri command 与 notification dispatcher 各自独占的 channel half。
    pub fn into_parts(self) -> (Sender<EditorRequestEnvelope>, Receiver<EditorNotification>) {
        (self.request_sender, self.notification_receiver)
    }
}
