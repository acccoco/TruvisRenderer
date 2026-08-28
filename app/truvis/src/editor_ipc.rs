//! Tauri WebView 到 RenderThread 的 Editor IPC owner。
//!
//! 本模块只负责 Tauri invoke、通知转发和请求超时；领域请求仍由 RenderThread 上的
//! `EditorController` 解释，CPU `World` 与 Vulkan 对象不会进入 desktop state。

use std::sync::Mutex;
use std::time::Duration;

use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc;

use truvis_editor_bridge::protocol::{EditorError, EditorErrorCode, EditorRequest, EditorResponse};
use truvis_editor_bridge::{EditorRequestEnvelope, FrontendEndpoint};

const EDITOR_REQUEST_TIMEOUT: Duration = Duration::from_secs(2);
const EDITOR_NOTIFICATION_EVENT: &str = "editor-notification";
const MAIN_WEBVIEW_LABEL: &str = "main";

/// Tauri desktop 独占的 Editor IPC 生命周期 owner。
///
/// request sender 可以被并发 invoke 共享；notification receiver 只由构造时启动的
/// async task 消费。task handle 放在 mutex 中仅用于幂等 shutdown，不跨 await 持锁。
pub(crate) struct EditorIpc {
    request_sender: mpsc::Sender<EditorRequestEnvelope>,
    notification_task: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
}

impl EditorIpc {
    pub(crate) fn start(app_handle: AppHandle, endpoint: FrontendEndpoint) -> Self {
        let (request_sender, mut notification_receiver) = endpoint.into_parts();
        let notification_task = tauri::async_runtime::spawn(async move {
            while let Some(notification) = notification_receiver.recv().await {
                if let Err(error) = app_handle.emit_to(MAIN_WEBVIEW_LABEL, EDITOR_NOTIFICATION_EVENT, notification) {
                    log::warn!("failed to emit Editor notification to Tauri WebView: {error}");
                }
            }
        });

        Self {
            request_sender,
            notification_task: Mutex::new(Some(notification_task)),
        }
    }

    /// 非阻塞提交请求，并异步等待 RenderThread 的独立 reply。
    ///
    /// inbox 背压、Renderer 关闭和 timeout 都收敛为现有 EditorResponse 错误，WebView
    /// 不需要维护 Tauri IPC 之外的第二套 pending/request-id 协议。
    pub(crate) fn request_sender(&self) -> mpsc::Sender<EditorRequestEnvelope> {
        self.request_sender.clone()
    }

    pub(crate) async fn request(
        request_sender: mpsc::Sender<EditorRequestEnvelope>,
        request: EditorRequest,
    ) -> EditorResponse {
        let (reply, reply_receiver) = tokio::sync::oneshot::channel();
        match request_sender.try_send(EditorRequestEnvelope { request, reply }) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => {
                return Self::error(EditorErrorCode::Busy, "editor request queue is full");
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                return Self::error(EditorErrorCode::Internal, "native renderer is not available");
            }
        }

        match tokio::time::timeout(EDITOR_REQUEST_TIMEOUT, reply_receiver).await {
            Ok(Ok(response)) => response,
            Ok(Err(_)) => {
                Self::error(EditorErrorCode::Internal, "native renderer stopped before completing the request")
            }
            Err(_) => Self::error(EditorErrorCode::Timeout, "editor request timed out"),
        }
    }

    pub(crate) fn shutdown(&self) {
        let mut task = self.notification_task.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(task) = task.take() {
            task.abort();
        }
    }

    pub(crate) fn unavailable(message: impl Into<String>) -> EditorResponse {
        Self::error(EditorErrorCode::Internal, message)
    }

    fn error(code: EditorErrorCode, message: impl Into<String>) -> EditorResponse {
        EditorResponse::Error(EditorError::new(code, message))
    }
}

impl Drop for EditorIpc {
    fn drop(&mut self) {
        let task = self.notification_task.get_mut().unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(task) = task.take() {
            task.abort();
        }
    }
}
