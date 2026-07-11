use std::sync::mpsc;
use std::thread;

use anyhow::{Context, Result, anyhow};
use tokio::runtime;
use tokio::sync::watch;

use truvis_editor_bridge::ServerEndpoint;

use crate::config::EditorServerConfig;
use crate::handle::EditorServerHandle;
use crate::runtime::EditorServerRuntime;

/// Editor Server 的创建入口。
///
/// 本类型不保存运行时状态；`start` 把网络 owner 移入专用线程，并在返回前等待 bind 结果，
/// 避免 App 在端口冲突后继续运行一个不可访问的 editor。
pub struct EditorServer;

impl EditorServer {
    pub fn start(config: EditorServerConfig, endpoint: ServerEndpoint) -> Result<EditorServerHandle> {
        if !config.bind_addr.ip().is_loopback() {
            return Err(anyhow!("EditorServer only accepts loopback bind addresses"));
        }
        if !config.web_root.join("index.html").is_file() {
            return Err(anyhow!("EditorServer web root does not contain index.html: {}", config.web_root.display()));
        }

        let (shutdown_sender, shutdown_receiver) = watch::channel(false);
        let (startup_sender, startup_receiver) = mpsc::sync_channel(1);
        let join_handle = thread::Builder::new()
            .name("EditorServer".to_string())
            .spawn(move || {
                let runtime = match runtime::Builder::new_current_thread().enable_all().build() {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        let _ = startup_sender.send(Err(error.to_string()));
                        return;
                    }
                };

                if let Err(error) =
                    runtime.block_on(EditorServerRuntime::run(config, endpoint, shutdown_receiver, startup_sender))
                {
                    log::error!("EditorServer stopped with error: {error:#}");
                }
            })
            .context("failed to spawn EditorServer thread")?;

        match startup_receiver.recv() {
            Ok(Ok(bound_addr)) => Ok(EditorServerHandle::new(bound_addr, shutdown_sender, join_handle)),
            Ok(Err(message)) => {
                let _ = join_handle.join();
                Err(anyhow!(message))
            }
            Err(error) => {
                let _ = join_handle.join();
                Err(anyhow!("EditorServer startup channel closed: {error}"))
            }
        }
    }
}
