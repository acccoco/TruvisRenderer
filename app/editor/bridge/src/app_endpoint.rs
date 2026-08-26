use tokio::sync::mpsc::error::{TryRecvError, TrySendError};
use tokio::sync::mpsc::{Receiver, Sender};

use crate::{EditorNotificationEnvelope, EditorRequestEnvelope, EditorResponseEnvelope};

/// RenderThread 上 Renderer 独占的跨线程 endpoint。
///
/// Renderer 每帧通过 `try_receive_request` 按预算消费请求，通过 `try_send_*` 非阻塞返回结果。
/// 任何方法都不会等待 Server 或持有跨线程锁。
pub struct AppEndpoint {
    request_receiver: Receiver<EditorRequestEnvelope>,
    response_sender: Sender<EditorResponseEnvelope>,
    notification_sender: Sender<EditorNotificationEnvelope>,
}

impl AppEndpoint {
    pub(crate) fn new(
        request_receiver: Receiver<EditorRequestEnvelope>,
        response_sender: Sender<EditorResponseEnvelope>,
        notification_sender: Sender<EditorNotificationEnvelope>,
    ) -> Self {
        Self {
            request_receiver,
            response_sender,
            notification_sender,
        }
    }

    /// 非阻塞读取一条 editor 请求。
    pub fn try_receive_request(&mut self) -> Result<EditorRequestEnvelope, TryRecvError> {
        self.request_receiver.try_recv()
    }

    /// 非阻塞发送请求响应；队列满或 Server 已关闭时把 envelope 返还给调用方。
    pub fn try_send_response(
        &self,
        response: EditorResponseEnvelope,
    ) -> Result<(), TrySendError<EditorResponseEnvelope>> {
        self.response_sender.try_send(response)
    }

    /// 非阻塞发送 best-effort notification。
    pub fn try_send_notification(
        &self,
        notification: EditorNotificationEnvelope,
    ) -> Result<(), TrySendError<EditorNotificationEnvelope>> {
        self.notification_sender.try_send(notification)
    }

    /// 关闭 request receiver，阻止 Server 继续排入新请求，同时允许 Renderer 丢弃剩余请求后退出。
    pub fn close_requests(&mut self) {
        self.request_receiver.close();
    }
}
