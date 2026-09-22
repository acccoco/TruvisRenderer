//! Truvis Web 编辑器与 Automation 的跨线程协议和有界 endpoint。

mod envelope;
mod frontend_endpoint;
pub mod protocol;
mod renderer_endpoint;

use tokio::sync::mpsc;

pub use envelope::RequestEnvelope;
pub use frontend_endpoint::FrontendEndpoint;
pub use renderer_endpoint::RendererEndpoint;

pub type EditorFrontendEndpoint =
    FrontendEndpoint<protocol::EditorRequest, protocol::EditorResponse, protocol::EditorNotification>;
pub type EditorRendererEndpoint =
    RendererEndpoint<protocol::EditorRequest, protocol::EditorResponse, protocol::EditorNotification>;
pub type AutomationFrontendEndpoint =
    FrontendEndpoint<protocol::AutomationRequest, protocol::AutomationResponse, protocol::AutomationNotification>;
pub type AutomationRendererEndpoint =
    RendererEndpoint<protocol::AutomationRequest, protocol::AutomationResponse, protocol::AutomationNotification>;
pub type EditorRequestEnvelope = RequestEnvelope<protocol::EditorRequest, protocol::EditorResponse>;
pub type EditorBridgeConfig = EndpointConfig;

/// Editor 与 Automation 共用的有界队列配置。
#[derive(Clone, Copy, Debug)]
pub struct EndpointConfig {
    pub request_capacity: usize,
    pub notification_capacity: usize,
}

impl Default for EndpointConfig {
    fn default() -> Self {
        Self {
            request_capacity: 256,
            notification_capacity: 64,
        }
    }
}

pub fn create_editor_bridge(config: EndpointConfig) -> (EditorFrontendEndpoint, EditorRendererEndpoint) {
    let (request_sender, request_receiver) = mpsc::channel(config.request_capacity);
    let (notification_sender, notification_receiver) = mpsc::channel(config.notification_capacity);
    (
        FrontendEndpoint::new(request_sender, notification_receiver),
        RendererEndpoint::new(request_receiver, notification_sender),
    )
}

pub fn create_automation_bridge(config: EndpointConfig) -> (AutomationFrontendEndpoint, AutomationRendererEndpoint) {
    let (request_sender, request_receiver) = mpsc::channel(config.request_capacity);
    let (notification_sender, notification_receiver) = mpsc::channel(config.notification_capacity);
    (
        FrontendEndpoint::new(request_sender, notification_receiver),
        RendererEndpoint::new(request_receiver, notification_sender),
    )
}
