use tokio::sync::oneshot;

/// Frontend 写入 Render request inbox 的单条请求。
#[derive(Debug)]
pub struct RequestEnvelope<Request, Response> {
    pub request: Request,
    pub reply: oneshot::Sender<Response>,
}
