use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;

/// Editor Server 的启动配置。
///
/// 第一阶段只允许 loopback 地址。`web_root` 指向显式构建的 Vite `dist/`，Cargo build
/// 不会隐式执行 npm。环境变量只用于开发和发布路径覆盖。
#[derive(Clone, Debug)]
pub struct EditorServerConfig {
    pub bind_addr: SocketAddr,
    pub web_root: PathBuf,
    pub max_websocket_message_size: usize,
    pub client_outbox_capacity: usize,
}

impl Default for EditorServerConfig {
    fn default() -> Self {
        let bind_addr = std::env::var("TRUVIS_EDITOR_ADDR")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or_else(|| SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 9473));
        let web_root = std::env::var_os("TRUVIS_EDITOR_WEB_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("app/editor/web/dist"));

        Self {
            bind_addr,
            web_root,
            max_websocket_message_size: 256 * 1024,
            client_outbox_capacity: 128,
        }
    }
}
