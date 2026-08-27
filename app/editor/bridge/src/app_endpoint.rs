use tokio::sync::mpsc::error::{TryRecvError, TrySendError};
use tokio::sync::mpsc::{Receiver, Sender};

use crate::EditorRequestEnvelope;
use crate::protocol::EditorNotification;

/// RenderThread 上 Renderer 独占的跨线程 endpoint。
///
/// Renderer 每帧通过 `try_receive_request` 按预算消费请求，并通过 request 自带的 oneshot
/// 非阻塞返回结果。任何方法都不会等待 Desktop 或持有跨线程锁。
pub struct AppEndpoint {
    request_receiver: Receiver<EditorRequestEnvelope>,
    notification_sender: Sender<EditorNotification>,
}

impl AppEndpoint {
    pub(crate) fn new(
        request_receiver: Receiver<EditorRequestEnvelope>,
        notification_sender: Sender<EditorNotification>,
    ) -> Self {
        Self {
            request_receiver,
            notification_sender,
        }
    }

    /// 非阻塞读取一条 editor 请求。
    pub fn try_receive_request(&mut self) -> Result<EditorRequestEnvelope, TryRecvError> {
        self.request_receiver.try_recv()
    }

    /// 非阻塞发送 best-effort notification。
    pub fn try_send_notification(
        &self,
        notification: EditorNotification,
    ) -> Result<(), TrySendError<EditorNotification>> {
        self.notification_sender.try_send(notification)
    }

    /// 关闭并清空 request inbox，使所有等待中的 Tauri invoke 立即观察到 reply sender drop。
    pub fn shutdown(&mut self) {
        self.request_receiver.close();
        while self.request_receiver.try_recv().is_ok() {}
    }
}
