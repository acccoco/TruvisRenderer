use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use truvis_path::TruvisPath;

/// 资源类型
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResourceType {
    /// 普通文件
    File,
    /// Zip 压缩包
    Zip,
    /// Git 仓库
    Git,
}

/// TOML 配置文件中的资源配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceConfig {
    /// 配置项列表
    pub resources: Vec<ResourceItem>,
}

/// 单个资源项配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceItem {
    /// 资源名称（用于日志输出）
    pub name: String,

    /// 完整的下载 URL
    pub url: String,

    /// 资源类型（file、zip 或 git）
    pub resource_type: ResourceType,

    /// 本地目标目录
    pub target_dir: String,

    /// 重命名
    /// - 如果 resource_type = File，则重命名文件
    /// - 如果 resource_type = Zip，则重命名解压后的顶级目录
    /// - 如果 resource_type = Git，则作为 checkout 目录名
    pub rename_to: String,

    /// Git resource 要 checkout 的 tag、branch 或 commit。
    /// 非 git resource 忽略该字段。
    #[serde(default)]
    pub git_ref: Option<String>,

    /// Git resource 是否初始化并更新 submodule。
    /// 非 git resource 忽略该字段。
    #[serde(default)]
    pub recursive_submodules: bool,

    /// 可选：是否强制重新下载，默认 false
    #[serde(default)]
    pub force_download: bool,

    #[serde(default)]
    pub force_overwrite: bool,
}

impl ResourceConfig {
    /// 从 TOML 文件加载配置
    pub fn from_file<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let content =
            fs::read_to_string(path.as_ref()).with_context(|| format!("读取配置文件失败: {:?}", path.as_ref()))?;

        let mut config: ResourceConfig =
            toml::from_str(&content).with_context(|| format!("解析 TOML 配置失败: {:?}", path.as_ref()))?;
        for item in &mut config.resources {
            item.target_dir = TruvisPath::workspace_path().join(&item.target_dir).to_str().unwrap().to_string();
        }

        Ok(config)
    }

    /// 保存配置到 TOML 文件（示例用途）
    pub fn save_to_file<P: AsRef<Path>>(&self, path: P) -> anyhow::Result<()> {
        let content = toml::to_string_pretty(self).context("序列化配置失败")?;

        fs::write(path.as_ref(), content).with_context(|| format!("写入配置文件失败: {:?}", path.as_ref()))?;

        Ok(())
    }
}
