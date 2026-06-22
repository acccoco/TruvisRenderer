//! 资源与工具下载 crate。
//!
//! 本 crate 读取根目录 `resources.toml`，按配置下载模型资产和外部工具资源，
//! 并把普通文件、zip 解压产物或 Git checkout 落到 workspace 内的目标目录。
//! Git resource 只接受 HTTP(S) URL，可按配置 checkout 指定 ref 并递归更新 submodule。
//! HTTP 下载和 Git 命令继承进程环境中的 proxy 变量，并对常见网络失败做超时与有限重试。
//! 路径解析依赖 `truvis-path`，日志格式由 `truvis-logs` 初始化。
//!
//! 可执行入口名为 `fetch_res`，根目录 `just fetch-res` 是推荐调用方式。
//! 入口默认把下载缓存放到 workspace 下的 `.temp/resource`，避免和日志、Streamline
//! 等其它临时文件混在 `.temp` 根目录。

pub mod fetch_resources;
