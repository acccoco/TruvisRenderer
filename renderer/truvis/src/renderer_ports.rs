//! Frontend 与 RenderThread renderer 的方向受限装配端口。
//!
//! 本模块只负责一次性创建并拆分通信 owner，不解释 Editor DTO，也不接触 Tauri API、
//! `World` 或 GPU 资源。frontend 端留在桌面主线程，renderer 端整体移入 Renderer factory。

use truvis_editor_bridge::{EditorBridgeConfig, FrontendEndpoint, RendererEndpoint, create_editor_bridge};

use crate::desktop_command::{DesktopCommandController, DesktopCommandSender};

/// Frontend adapter 独占的通信端口。
pub struct TruvisFrontendPorts {
    /// 通用 Editor request / notification endpoint。
    pub editor: FrontendEndpoint,

    /// 仅用于本地桌面特权操作的进程内命令提交端。
    pub desktop_commands: DesktopCommandSender,
}

/// RenderThread 上 [`crate::truvis_renderer::TruvisRenderer`] 独占的通信端口。
pub struct TruvisRendererPorts {
    pub(crate) editor: RendererEndpoint,
    pub(crate) desktop_commands: DesktopCommandController,
}

/// 一次性创建 Truvis frontend / renderer 双侧端口。
///
/// Editor bridge 容量来自调用方配置；desktop command 继续使用容量为一的独立队列。
pub fn create_truvis_ports(config: EditorBridgeConfig) -> (TruvisFrontendPorts, TruvisRendererPorts) {
    let (frontend_editor, renderer_editor) = create_editor_bridge(config);
    let (desktop_command_sender, desktop_command_controller) = DesktopCommandController::create();

    (
        TruvisFrontendPorts {
            editor: frontend_editor,
            desktop_commands: desktop_command_sender,
        },
        TruvisRendererPorts {
            editor: renderer_editor,
            desktop_commands: desktop_command_controller,
        },
    )
}
