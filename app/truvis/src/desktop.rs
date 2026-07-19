//! Truvis 主体应用的 Tauri 桌面壳。
//!
//! 本模块拥有顶层 Tauri window、EditorServer 与 embedded winit host 的组装和关闭
//! 顺序。它不处理材质领域命令，也不访问 Vulkan；editor DTO 到 `World` 的翻译仍由
//! RenderThread 上的 `EditorController` 完成。

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result};
use raw_window_handle::HasWindowHandle;
use serde::Deserialize;
use tauri::{Manager, RunEvent, State, WindowEvent};

use truvis_app_frame::{RenderAppShell, SendWrapper, init_env_with_log_file};
use truvis_editor_bridge::{EditorBridgeConfig, create_editor_bridge};
use truvis_editor_server::{EditorServer, EditorServerConfig, EditorServerHandle};
use truvis_logs::LogFilePath;
use truvis_path::TruvisPath;
use truvis_winit_app::embedded::{EmbeddedViewportRect, EmbeddedWinitHost};

use crate::truvis_app::TruvisApp;

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
/// 本状态只保存窗口宿主、Server handle 和网络入口，不保存 scene/material/selection
/// 投影。`shutting_down` 保证重复 close/exit 事件不会重复 join 同一线程。
struct TruvisDesktopState {
    resources: Mutex<TruvisDesktopResources>,
    editor_websocket_url: String,
    shutting_down: AtomicBool,
}

impl TruvisDesktopState {
    fn new(render_host: EmbeddedWinitHost, editor_server: EditorServerHandle, editor_websocket_url: String) -> Self {
        Self {
            resources: Mutex::new(TruvisDesktopResources {
                render_host: Some(render_host),
                editor_server: Some(editor_server),
            }),
            editor_websocket_url,
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
        let editor_server = EditorServer::start(EditorServerConfig::default(), server_endpoint)
            .context("failed to start embedded EditorServer")?;
        let websocket_url = format!("ws://{}/api/editor/v1/ws", editor_server.bound_addr());

        let app = tauri::Builder::default()
            .invoke_handler(tauri::generate_handler![set_render_viewport_rect, editor_websocket_url])
            .setup(move |app| {
                let window = app
                    .get_webview_window("main")
                    .ok_or_else(|| std::io::Error::other("Tauri main window was not created"))?;
                let parent_window = window
                    .window_handle()
                    .map_err(|error| std::io::Error::other(format!("failed to get Tauri parent HWND: {error}")))?
                    .as_raw();
                let render_host = EmbeddedWinitHost::spawn(SendWrapper(parent_window), move || {
                    Box::new(RenderAppShell::new(TruvisApp::new(app_endpoint)))
                })
                .map_err(std::io::Error::other)?;

                app.manage(TruvisDesktopState::new(render_host, editor_server, websocket_url));
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
