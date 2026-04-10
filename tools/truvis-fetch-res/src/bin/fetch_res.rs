//! GitHub 资源下载示例

use anyhow::Result;
use truvis_fetch_res::fetch_resources::resource_fetcher::GitHubResourceFetcher;
use truvis_logs::init_log;
use truvis_path::TruvisPath;

fn main() -> Result<()> {
    // 初始化日志
    init_log();

    let fetcher = GitHubResourceFetcher::with_temp_dir(TruvisPath::temp_dir())?;

    let config_path = TruvisPath::workspace_path().join("resources.toml");

    // 假设配置文件存在
    if config_path.exists() {
        fetcher.fetch_from_config(config_path)?;
        println!("✓ 批量下载完成");
    } else {
        println!("⚠ 配置文件 resources.toml 不存在");
    }

    Ok(())
}
