use tokio::sync::mpsc::{Receiver, Sender};

use crate::RequestEnvelope;

/// Frontend adapter 独占的跨线程 endpoint。
///
/// request sender 供 frontend command clone 后非阻塞提交请求；notification receiver 由
/// frontend 侧单个 async dispatcher 独占。该类型不保存 scene、selection 或 material 状态，
/// 也不依赖 Tauri、WebView 或其他具体传输实现。
pub struct FrontendEndpoint<Request, Response, Notification> {
    request_sender: Sender<RequestEnvelope<Request, Response>>,
    notification_receiver: Receiver<Notification>,
}

impl<Request, Response, Notification> FrontendEndpoint<Request, Response, Notification> {
    pub(crate) fn new(
        request_sender: Sender<RequestEnvelope<Request, Response>>,
        notification_receiver: Receiver<Notification>,
    ) -> Self {
        Self {
            request_sender,
            notification_receiver,
        }
    }

    /// 把 endpoint 拆成 Tauri command 与 notification dispatcher 各自独占的 channel half。
    pub fn into_parts(self) -> (Sender<RequestEnvelope<Request, Response>>, Receiver<Notification>) {
        (self.request_sender, self.notification_receiver)
    }
}
