//! Truvis 工具集
//!
//! 提供日志初始化、资源路径管理、命名数组等通用工具。
//!
//! # TruvisPath
//! 基于工作区根目录的统一路径管理，避免硬编码相对路径。
//!
//! # GitHub 资源下载
//! 支持从 GitHub 下载 zip 文件并解压，可通过 TOML 配置管理。

pub mod fetch_resources;
