//! 资源与工具下载 crate。
//!
//! 本 crate 读取根目录 `resources.toml`，按配置下载模型资产和外部工具资源，
//! 并把 zip 或普通文件落到 workspace 内的目标目录。路径解析依赖 `truvis-path`，
//! 日志格式由 `truvis-logs` 初始化。
//!
//! 可执行入口名为 `fetch_res`，根目录 `just fetch-res` 是推荐调用方式。

pub mod fetch_resources;
