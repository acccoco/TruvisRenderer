use tokio::sync::mpsc::{Receiver, Sender};

use crate::{EditorNotificationEnvelope, EditorRequestEnvelope, EditorResponseEnvelope};

/// Editor Server 独占的跨线程 endpoint。
///
/// request sender 可以克隆给多个 WebSocket task；response / notification receiver 必须由
/// Server 的单个 dispatcher 独占。该类型不保存 client 或场景状态。
pub struct ServerEndpoint {
    request_sender: Sender<EditorRequestEnvelope>,
    response_receiver: Receiver<EditorResponseEnvelope>,
    notification_receiver: Receiver<EditorNotificationEnvelope>,
}

impl ServerEndpoint {
    pub(crate) fn new(
        request_sender: Sender<EditorRequestEnvelope>,
        response_receiver: Receiver<EditorResponseEnvelope>,
        notification_receiver: Receiver<EditorNotificationEnvelope>,
    ) -> Self {
        Self {
            request_sender,
            response_receiver,
            notification_receiver,
        }
    }

    /// 把 endpoint 拆成 Server runtime 使用的单向 channel halves。
    pub fn into_parts(
        self,
    ) -> (Sender<EditorRequestEnvelope>, Receiver<EditorResponseEnvelope>, Receiver<EditorNotificationEnvelope>) {
        (self.request_sender, self.response_receiver, self.notification_receiver)
    }
}
