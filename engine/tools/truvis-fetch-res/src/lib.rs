//! 资源与工具下载 crate。
//!
//! 本 crate 读取根目录 `resources.toml`，按配置下载模型资产和外部工具资源，
//! 并把普通文件、zip/7z 解压产物或 Git checkout 落到 workspace 内的目标目录。
//! 批量入口默认只处理 `download_by_default = true` 的资源，也支持按唯一 `name`
//! 选择资源或显式处理全部配置项。
//! Git resource 只接受 HTTP(S) URL，可按配置 checkout 指定 ref 并递归更新 submodule。
//! 浮动 ref 先 fetch 再比较 commit；固定 commit/tag 完整匹配时跳过，普通更新复用 checkout。
//! HTTP 下载和 Git 命令继承进程环境中的 proxy 变量，并对常见网络失败做超时与有限重试。
//! 路径解析依赖 `truvis-path`，日志格式由 `truvis-logs` 初始化。
//!
//! 可执行入口名为 `fetch_res`，根目录 `just fetch-res` 是推荐调用方式。
//! 入口默认把下载缓存放到 workspace 下的 `.temp/resource`，避免和日志、Streamline
//! 等其它临时文件混在 `.temp` 根目录。
//! ZIP/7z 共用条目路径校验，先写入缓存目录下的资源 staging，完整成功后才替换目标；7z 使用纯 Rust 解压。
//! 解压失败保留旧资源并清理 staging；不支持多进程同时刷新同一资源。

pub mod fetch_resources;
