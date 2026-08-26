//! Truvis 主体应用的 Tauri 桌面壳。
//!
//! 本模块拥有顶层 Tauri window、EditorServer 与 embedded winit host 的组装和关闭
//! 顺序。它不处理材质领域命令，也不访问 Vulkan；editor DTO 与本地桌面特权命令
//! 都只能在 RenderThread 上各自的 controller 中进入权威 `World`。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use raw_window_handle::HasWindowHandle;
use serde::{Deserialize, Serialize};
use tauri::{Manager, RunEvent, State, WindowEvent};
use tauri_plugin_dialog::DialogExt;
use tokio::sync::oneshot;

use truvis_editor_bridge::{EditorBridgeConfig, create_editor_bridge};
use truvis_editor_server::{EditorServer, EditorServerConfig, EditorServerHandle};
use truvis_logs::LogFilePath;
use truvis_path::TruvisPath;
use truvis_render_loop::init_env_with_log_file;
use truvis_winit_host::{EmbeddedViewportRect, EmbeddedWinitHost};

use crate::desktop_command::{DesktopCommandController, DesktopCommandSender};
use crate::truvis_renderer::TruvisRenderer;

/// Tauri command 使用的 DOM viewport 物理像素矩形。
///
/// 前端负责把 `getBoundingClientRect()` 的 CSS pixel 乘以当前
/// `devicePixelRatio`；Rust 侧只校验整数范围并把窗口几何交给平台宿主。
#[derive(Clone, Copy, Debug, Deserialize)]
pub struct RenderViewportRect {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

impl From<RenderViewportRect> for EmbeddedViewportRect {
    fn from(value: RenderViewportRect) -> Self {
        Self {
            x: value.x,
            y: value.y,
            width: value.width,
            height: value.height,
        }
    }
}

/// 需要严格按顺序关闭的桌面资源集合。
///
/// `render_host` 必须先退出，使 Vulkan surface 与 child HWND 都在 Tauri parent
/// HWND 存活期间销毁；随后才停止 EditorServer。字段只在持有外层 mutex 时 take，
/// 实际 join 在锁外执行，避免 Tauri command 因长时间持锁而阻塞。
struct TruvisDesktopResources {
    render_host: Option<EmbeddedWinitHost>,
    editor_server: Option<EditorServerHandle>,
}

/// Tauri 主线程和 command handler 共享的桌面生命周期状态。
///
/// 本状态只保存窗口宿主、Server handle、网络入口和进程内 command sender，不保存
/// scene/material/selection 投影。`shutting_down` 保证重复 close/exit 事件不会重复
/// join 同一线程。
struct TruvisDesktopState {
    /// 严格按 RenderThread、EditorServer 顺序关闭的桌面资源集合。
    resources: Mutex<TruvisDesktopResources>,

    /// WebView 建立 Editor WebSocket 时读取的 loopback 地址。
    editor_websocket_url: String,

    /// Tauri command 向 RenderThread 提交本地特权命令的进程内 sender。
    desktop_command_sender: DesktopCommandSender,

    /// 防止多个原生 HDRI 文件对话框或待确认请求同时存在。
    hdri_dialog_open: Arc<AtomicBool>,

    /// 标记桌面资源已经进入 shutdown，阻止新 command 跨入正在销毁的线程。
    shutting_down: AtomicBool,
}

impl TruvisDesktopState {
    fn new(
        render_host: EmbeddedWinitHost,
        editor_server: EditorServerHandle,
        editor_websocket_url: String,
        desktop_command_sender: DesktopCommandSender,
    ) -> Self {
        Self {
            resources: Mutex::new(TruvisDesktopResources {
                render_host: Some(render_host),
                editor_server: Some(editor_server),
            }),
            editor_websocket_url,
            desktop_command_sender,
            hdri_dialog_open: Arc::new(AtomicBool::new(false)),
            shutting_down: AtomicBool::new(false),
        }
    }

    fn set_viewport_rect(&self, rect: RenderViewportRect) -> std::result::Result<(), String> {
        if self.shutting_down.load(Ordering::Acquire) {
            return Err("Truvis desktop is shutting down".to_string());
        }
        let resources = self.resources.lock().map_err(|_| "Truvis desktop resource lock is poisoned".to_string())?;
        let render_host =
            resources.render_host.as_ref().ok_or_else(|| "embedded render host is not available".to_string())?;
        render_host.set_viewport_rect(rect.into())
    }

    fn take_resources(&self) -> Option<TruvisDesktopResources> {
        if self.shutting_down.swap(true, Ordering::AcqRel) {
            return None;
        }
        let mut resources = self.resources.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        Some(TruvisDesktopResources {
            render_host: resources.render_host.take(),
            editor_server: resources.editor_server.take(),
        })
    }

    /// 获取一次独占的 HDRI 对话框许可。
    ///
    /// permit 使用 RAII 归还原子标记，因此 command future 被取消、用户取消选择或
    /// RenderThread reply 失败都不会让后续文件选择永久处于 busy。
    fn begin_hdri_dialog(&self) -> std::result::Result<HdriDialogPermit, String> {
        if self.shutting_down.load(Ordering::Acquire) {
            return Err("Truvis desktop is shutting down".to_string());
        }
        self.hdri_dialog_open
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| "An HDRI selection is already in progress".to_string())?;
        Ok(HdriDialogPermit {
            dialog_open: Arc::clone(&self.hdri_dialog_open),
        })
    }

    /// clone 轻量 sender，使 async command 不必跨 await 持有 Tauri `State` guard。
    fn desktop_command_sender(&self) -> DesktopCommandSender {
        self.desktop_command_sender.clone()
    }

    /// 对原生 dialog 结果执行最终 App 入口校验。
    ///
    /// dialog filter 只改善用户体验，不能作为输入约束；这里再次检查扩展名，确保私有
    /// RenderThread command 只接收本功能承诺的 Radiance HDR 或 OpenEXR 文件。
    fn validate_hdri_path(path: &Path) -> std::result::Result<(), String> {
        let supported = path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("hdr") || extension.eq_ignore_ascii_case("exr"));
        if !supported {
            return Err("Selected file must use the .hdr or .exr extension".to_string());
        }
        Ok(())
    }

    /// 关闭顺序是本集成最重要的生命周期不变量：RenderThread → child HWND →
    /// EditorServer → Tauri parent HWND。
    fn shutdown(&self) {
        let Some(mut resources) = self.take_resources() else {
            return;
        };
        if let Some(render_host) = resources.render_host.take() {
            if let Err(error) = render_host.shutdown() {
                log::error!("failed to shut down embedded render host cleanly: {error}");
            }
        }
        if let Some(mut editor_server) = resources.editor_server.take() {
            editor_server.shutdown();
        }
    }
}

/// 一次原生 HDRI 文件选择及其 RenderThread 接受过程的独占许可。
///
/// permit 完整覆盖 dialog 和 reply 等待期；只要 command future 结束，`Drop` 就恢复
/// `hdri_dialog_open`，不依赖每个错误分支手工清理。
struct HdriDialogPermit {
    /// 指向 `TruvisDesktopState` 中并发保护标记的共享引用。
    dialog_open: Arc<AtomicBool>,
}

impl Drop for HdriDialogPermit {
    fn drop(&mut self) {
        self.dialog_open.store(false, Ordering::Release);
    }
}

/// `select_hdri` 返回给 WebView 的最小结果。
///
/// 完整路径不会跨越 Rust/Tauri IPC；`Accepted` 只确认 CPU scene 已接受请求，不代表
/// 后台 decode、GPU upload 或 sky distribution 已完成。
#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum SelectHdriResult {
    /// 用户主动关闭文件对话框，scene 没有发生变化。
    Cancelled,

    /// RenderThread 已接受 sky texture 请求。
    Accepted {
        /// 仅供 TopBar 展示的文件名，不包含本机目录。
        file_name: String,
    },
}

#[tauri::command]
fn set_render_viewport_rect(
    rect: RenderViewportRect,
    state: State<'_, TruvisDesktopState>,
) -> std::result::Result<(), String> {
    state.set_viewport_rect(rect)
}

#[tauri::command]
fn editor_websocket_url(state: State<'_, TruvisDesktopState>) -> String {
    state.editor_websocket_url.clone()
}

/// 打开原生 HDRI 文件选择器，并等待 RenderThread 接受 scene mutation。
///
/// 文件对话框使用 callback API，避免阻塞 Tauri main event loop；选中的 `PathBuf`
/// 随后只经过进程内有界队列。这里等待的是 CPU scene 结果，不等待任何 asset/GPU 阶段。
#[tauri::command]
async fn select_hdri(
    app: tauri::AppHandle,
    state: State<'_, TruvisDesktopState>,
) -> std::result::Result<SelectHdriResult, String> {
    let _dialog_permit = state.begin_hdri_dialog()?;
    let desktop_command_sender = state.desktop_command_sender();
    drop(state);

    let (selection_sender, selection_receiver) = oneshot::channel();
    let mut dialog =
        app.dialog().file().set_title("Choose HDR Environment").add_filter("HDR Environment", &["hdr", "exr"]);
    if let Some(window) = app.get_webview_window("main") {
        dialog = dialog.set_parent(&window);
    }
    dialog.pick_file(move |selection| {
        let _ = selection_sender.send(selection);
    });

    let selection = selection_receiver.await.map_err(|_| "HDRI file dialog did not return a result".to_string())?;
    let Some(selection) = selection else {
        return Ok(SelectHdriResult::Cancelled);
    };
    let path: PathBuf =
        selection.into_path().map_err(|_| "Selected HDRI is not available as a local filesystem path".to_string())?;
    TruvisDesktopState::validate_hdri_path(&path)?;
    let file_name =
        path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_else(|| "Selected HDRI".to_string());

    let reply = desktop_command_sender.try_request_sky_texture(path)?;
    reply.await.map_err(|_| "Native renderer stopped before accepting the HDRI request".to_string())??;
    Ok(SelectHdriResult::Accepted { file_name })
}

/// 主体应用的 Tauri/Tao 进程入口。
///
/// Tauri 保持在进程 main thread；`EmbeddedWinitHost` 创建独立
/// `RenderWindowThread`，后者再启动现有 `RenderThread`。EditorServer 在 WebView
/// 显示前完成 bind，因此嵌入页面首次连接不会与 Server 启动竞争。
pub struct TruvisDesktop;

impl TruvisDesktop {
    pub fn run() -> Result<()> {
        init_env_with_log_file(LogFilePath::current_exe(TruvisPath::temp_dir()));

        let (server_endpoint, app_endpoint) = create_editor_bridge(EditorBridgeConfig::default());
        let (desktop_command_sender, desktop_command_controller) = DesktopCommandController::create();
        let editor_server = EditorServer::start(EditorServerConfig::default(), server_endpoint)
            .context("failed to start embedded EditorServer")?;
        let websocket_url = format!("ws://{}/api/editor/v1/ws", editor_server.bound_addr());

        let app = tauri::Builder::default()
            .plugin(tauri_plugin_dialog::init())
            .invoke_handler(tauri::generate_handler![set_render_viewport_rect, editor_websocket_url, select_hdri])
            .setup(move |app| {
                let window = app
                    .get_webview_window("main")
                    .ok_or_else(|| std::io::Error::other("Tauri main window was not created"))?;
                let parent_window = window
                    .window_handle()
                    .map_err(|error| std::io::Error::other(format!("failed to get Tauri parent HWND: {error}")))?
                    .as_raw();
                let render_host = EmbeddedWinitHost::spawn(parent_window, move || {
                    Box::new(TruvisRenderer::new(app_endpoint, desktop_command_controller))
                })
                .map_err(std::io::Error::other)?;

                app.manage(TruvisDesktopState::new(render_host, editor_server, websocket_url, desktop_command_sender));
                window.show()?;
                Ok(())
            })
            .build(tauri::generate_context!())
            .context("failed to build Tauri desktop application")?;

        app.run(|app_handle, event| match event {
            RunEvent::WindowEvent {
                label,
                event: WindowEvent::CloseRequested { api, .. },
                ..
            } => {
                if label == "main" {
                    api.prevent_close();
                    let state = app_handle.state::<TruvisDesktopState>();
                    state.shutdown();
                    app_handle.exit(0);
                }
            }
            // 覆盖程序化 exit/restart 等非窗口关闭路径。正常 CloseRequested 已经
            // take 过资源，因此这里是幂等 no-op。
            RunEvent::ExitRequested { .. } => {
                if let Some(state) = app_handle.try_state::<TruvisDesktopState>() {
                    state.shutdown();
                }
            }
            _ => {}
        });
        Ok(())
    }
}
