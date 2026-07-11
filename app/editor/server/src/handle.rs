use std::net::SocketAddr;
use std::thread::JoinHandle;

use tokio::sync::watch;

/// 已启动 Editor Server 的生命周期 handle。
///
/// App 是该 handle 的唯一 owner。`shutdown` 会先通知 HTTP accept loop、WebSocket connection
/// 和 response dispatcher 退出，再 join 专用 Server 线程；Drop 仅作为异常路径兜底。
pub struct EditorServerHandle {
    bound_addr: SocketAddr,
    shutdown_sender: watch::Sender<bool>,
    join_handle: Option<JoinHandle<()>>,
}

impl EditorServerHandle {
    pub(crate) fn new(
        bound_addr: SocketAddr,
        shutdown_sender: watch::Sender<bool>,
        join_handle: JoinHandle<()>,
    ) -> Self {
        Self {
            bound_addr,
            shutdown_sender,
            join_handle: Some(join_handle),
        }
    }

    /// 返回实际监听地址，支持未来配置端口 0 后查询系统分配端口。
    pub fn bound_addr(&self) -> SocketAddr {
        self.bound_addr
    }

    /// 请求停止并等待 Server 线程退出。
    pub fn shutdown(&mut self) {
        let _ = self.shutdown_sender.send(true);
        if let Some(join_handle) = self.join_handle.take() {
            if join_handle.join().is_err() {
                log::error!("EditorServer thread panicked during shutdown");
            }
        }
    }
}

impl Drop for EditorServerHandle {
    fn drop(&mut self) {
        self.shutdown();
    }
}
