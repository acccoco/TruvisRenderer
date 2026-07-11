use std::collections::HashMap;
use std::fmt::Display;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::SyncSender;

use anyhow::{Context, Result};
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use tokio::net::TcpListener;
use tokio::sync::{RwLock, mpsc, watch};
use tower_http::services::ServeDir;

use truvis_editor_bridge::protocol::{
    ClientId, EDITOR_PROTOCOL_VERSION, EditorClientMessage, EditorError, EditorErrorCode, EditorResponse,
    EditorServerMessage, RequestId,
};
use truvis_editor_bridge::{
    EditorNotificationEnvelope, EditorNotificationTarget, EditorRequestEnvelope, EditorResponseEnvelope, ServerEndpoint,
};

use crate::config::EditorServerConfig;

type ClientSender = mpsc::Sender<EditorServerMessage>;

/// Axum handler 与 response dispatcher 共享的纯通信状态。
///
/// clients map 只保存 WebSocket outbox，不保存 selection、scene 或 material。request sender
/// 是 Server → Render 的唯一领域消息出口。
struct EditorServerState {
    request_sender: mpsc::Sender<EditorRequestEnvelope>,
    clients: RwLock<HashMap<ClientId, ClientSender>>,
    next_client_id: AtomicU64,
    shutdown_receiver: watch::Receiver<bool>,
    max_websocket_message_size: usize,
    client_outbox_capacity: usize,
}

impl EditorServerState {
    fn allocate_client_id(&self) -> ClientId {
        ClientId(self.next_client_id.fetch_add(1, Ordering::Relaxed))
    }

    async fn send_to_client(&self, client_id: ClientId, message: EditorServerMessage) {
        let sender = self.clients.read().await.get(&client_id).cloned();
        if let Some(sender) = sender {
            let _ = sender.try_send(message);
        }
    }

    async fn broadcast(&self, message: EditorServerMessage) {
        let senders = self.clients.read().await.values().cloned().collect::<Vec<_>>();
        for sender in senders {
            let _ = sender.try_send(message.clone());
        }
    }
}

/// 专用线程中运行的 Axum/Tokio owner。
///
/// 该类型拥有 listener、client routing 和跨线程 outbox receiver；`run` 返回后这些网络资源
/// 全部位于 Server 线程内完成销毁。
pub(crate) struct EditorServerRuntime;

impl EditorServerRuntime {
    pub(crate) async fn run(
        config: EditorServerConfig,
        endpoint: ServerEndpoint,
        shutdown_receiver: watch::Receiver<bool>,
        startup_sender: SyncSender<std::result::Result<SocketAddr, String>>,
    ) -> Result<()> {
        let listener = match TcpListener::bind(config.bind_addr).await {
            Ok(listener) => listener,
            Err(error) => {
                let _ = startup_sender.send(Err(error.to_string()));
                return Ok(());
            }
        };
        let bound_addr = listener.local_addr().context("failed to read EditorServer bound address")?;
        let _ = startup_sender.send(Ok(bound_addr));

        let (request_sender, response_receiver, notification_receiver) = endpoint.into_parts();
        let state = Arc::new(EditorServerState {
            request_sender,
            clients: RwLock::new(HashMap::new()),
            next_client_id: AtomicU64::new(1),
            shutdown_receiver: shutdown_receiver.clone(),
            max_websocket_message_size: config.max_websocket_message_size,
            client_outbox_capacity: config.client_outbox_capacity,
        });

        let dispatcher = tokio::spawn(Self::dispatch_outbox(
            state.clone(),
            response_receiver,
            notification_receiver,
            shutdown_receiver.clone(),
        ));
        let router = Router::new()
            .route("/api/editor/v1/health", get(Self::health))
            .route("/api/editor/v1/info", get(Self::info))
            .route("/api/editor/v1/ws", get(Self::websocket_upgrade))
            .fallback_service(ServeDir::new(config.web_root).append_index_html_on_directories(true))
            .with_state(state);

        log::info!("EditorServer listening on http://{bound_addr}");
        axum::serve(listener, router)
            .with_graceful_shutdown(Self::wait_for_shutdown(shutdown_receiver))
            .await
            .context("EditorServer serve loop failed")?;

        dispatcher.abort();
        let _ = dispatcher.await;
        Ok(())
    }

    async fn dispatch_outbox(
        state: Arc<EditorServerState>,
        mut response_receiver: mpsc::Receiver<EditorResponseEnvelope>,
        mut notification_receiver: mpsc::Receiver<EditorNotificationEnvelope>,
        mut shutdown_receiver: watch::Receiver<bool>,
    ) {
        loop {
            tokio::select! {
                changed = shutdown_receiver.changed() => {
                    if changed.is_err() || *shutdown_receiver.borrow() {
                        break;
                    }
                }
                response = response_receiver.recv() => {
                    let Some(response) = response else { break };
                    state.send_to_client(response.client_id, EditorServerMessage::Response {
                        request_id: response.request_id,
                        response: response.response,
                    }).await;
                }
                notification = notification_receiver.recv() => {
                    let Some(notification) = notification else { break };
                    let message = EditorServerMessage::Notification {
                        notification: notification.notification,
                    };
                    match notification.target {
                        EditorNotificationTarget::Broadcast => state.broadcast(message).await,
                        EditorNotificationTarget::Client(client_id) => state.send_to_client(client_id, message).await,
                    }
                }
            }
        }
    }

    async fn health() -> StatusCode {
        StatusCode::NO_CONTENT
    }

    async fn info() -> Json<EditorServerInfo> {
        Json(EditorServerInfo {
            protocol_version: EDITOR_PROTOCOL_VERSION,
            websocket_path: "/api/editor/v1/ws",
        })
    }

    async fn websocket_upgrade(
        State(state): State<Arc<EditorServerState>>,
        headers: HeaderMap,
        websocket: WebSocketUpgrade,
    ) -> Response {
        if !Self::origin_is_allowed(&headers) {
            return StatusCode::FORBIDDEN.into_response();
        }

        websocket
            .max_message_size(state.max_websocket_message_size)
            .on_upgrade(move |socket| Self::serve_websocket(state, socket))
            .into_response()
    }

    fn origin_is_allowed(headers: &HeaderMap) -> bool {
        let Some(origin) = headers.get("origin") else {
            return true;
        };
        let Ok(origin) = origin.to_str() else {
            return false;
        };
        origin.starts_with("http://127.0.0.1:")
            || origin.starts_with("http://localhost:")
            || origin.starts_with("http://[::1]:")
    }

    async fn serve_websocket(state: Arc<EditorServerState>, socket: WebSocket) {
        let client_id = state.allocate_client_id();
        let (client_sender, mut client_receiver) = mpsc::channel(state.client_outbox_capacity);
        state.clients.write().await.insert(client_id, client_sender);

        let (mut socket_sender, mut socket_receiver) = socket.split();
        let mut shutdown_receiver = state.shutdown_receiver.clone();
        loop {
            tokio::select! {
                changed = shutdown_receiver.changed() => {
                    if changed.is_err() || *shutdown_receiver.borrow() {
                        let _ = socket_sender.send(Message::Close(None)).await;
                        break;
                    }
                }
                outbound = client_receiver.recv() => {
                    let Some(outbound) = outbound else { break };
                    if Self::send_websocket_message(&mut socket_sender, &outbound).await.is_err() {
                        break;
                    }
                }
                inbound = socket_receiver.next() => {
                    let Some(Ok(inbound)) = inbound else { break };
                    if !Self::handle_client_message(&state, client_id, &mut socket_sender, inbound).await {
                        break;
                    }
                }
            }
        }

        state.clients.write().await.remove(&client_id);
    }

    async fn handle_client_message<S>(
        state: &Arc<EditorServerState>,
        client_id: ClientId,
        socket_sender: &mut S,
        message: Message,
    ) -> bool
    where
        S: futures_util::Sink<Message> + Unpin,
        S::Error: Display,
    {
        let text = match message {
            Message::Text(text) => text,
            Message::Close(_) => {
                // tungstenite 在读取 Close frame 时已经把应答加入内部队列；这里显式 flush，
                // 再退出 client task，避免页面刷新被浏览器识别为异常断线。
                let _ = socket_sender.flush().await;
                return false;
            }
            Message::Binary(_) | Message::Ping(_) | Message::Pong(_) => return true,
        };

        let client_message = match serde_json::from_str::<EditorClientMessage>(&text) {
            Ok(message) => message,
            Err(error) => {
                let response = Self::error_message(
                    RequestId::new("invalid"),
                    EditorErrorCode::InvalidRequest,
                    format!("invalid editor message: {error}"),
                );
                return Self::send_websocket_message(socket_sender, &response).await.is_ok();
            }
        };
        if client_message.protocol_version != EDITOR_PROTOCOL_VERSION {
            let response = Self::error_message(
                client_message.request_id,
                EditorErrorCode::UnsupportedProtocol,
                format!("expected editor protocol version {EDITOR_PROTOCOL_VERSION}"),
            );
            return Self::send_websocket_message(socket_sender, &response).await.is_ok();
        }

        let envelope = EditorRequestEnvelope {
            client_id,
            request_id: client_message.request_id.clone(),
            request: client_message.request,
        };
        if state.request_sender.try_send(envelope).is_err() {
            let response =
                Self::error_message(client_message.request_id, EditorErrorCode::Busy, "render request inbox is full");
            return Self::send_websocket_message(socket_sender, &response).await.is_ok();
        }
        true
    }

    fn error_message(request_id: RequestId, code: EditorErrorCode, message: impl Into<String>) -> EditorServerMessage {
        EditorServerMessage::Response {
            request_id,
            response: EditorResponse::Error(EditorError::new(code, message)),
        }
    }

    async fn send_websocket_message<S>(sender: &mut S, message: &EditorServerMessage) -> std::result::Result<(), String>
    where
        S: futures_util::Sink<Message> + Unpin,
        S::Error: Display,
    {
        let json = serde_json::to_string(message).map_err(|error| error.to_string())?;
        sender.send(Message::Text(json.into())).await.map_err(|error| error.to_string())
    }

    async fn wait_for_shutdown(mut shutdown_receiver: watch::Receiver<bool>) {
        if *shutdown_receiver.borrow() {
            return;
        }
        let _ = shutdown_receiver.changed().await;
    }
}

/// `/api/editor/v1/info` 的固定 transport 信息。
#[derive(Serialize)]
struct EditorServerInfo {
    protocol_version: u32,
    websocket_path: &'static str,
}
