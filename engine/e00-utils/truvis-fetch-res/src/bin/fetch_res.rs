//! GitHub 资源下载示例

use std::collections::BTreeSet;
use std::env;

use anyhow::Result;

use truvis_fetch_res::fetch_resources::resource_fetcher::{FetchSelection, GitHubResourceFetcher};
use truvis_logs::{LogFilePath, TruvisLogger};
use truvis_path::TruvisPath;

struct CliOptions {
    selection: FetchSelection,
    verbose: bool,
}

fn main() -> Result<()> {
    let Some(options) = parse_selection()? else {
        print_help();
        return Ok(());
    };

    TruvisLogger::init_tool_with_file(LogFilePath::current_exe(TruvisPath::temp_dir()), options.verbose);

    let resource_temp_dir = TruvisPath::temp_dir().join("resource");
    let fetcher = GitHubResourceFetcher::with_temp_dir(resource_temp_dir)?;

    let config_path = TruvisPath::workspace_path().join("resources.toml");

    // 假设配置文件存在
    if config_path.exists() {
        fetcher.fetch_from_config_with_selection(config_path, options.selection)?;
        println!("✓ 批量下载完成");
    } else {
        println!("⚠ 配置文件 resources.toml 不存在");
    }

    Ok(())
}

fn parse_selection() -> Result<Option<CliOptions>> {
    let mut args = env::args().skip(1).collect::<Vec<_>>();

    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        return Ok(None);
    }

    let verbose = args.iter().any(|arg| arg == "--verbose");
    args.retain(|arg| arg != "--verbose");

    if args.iter().any(|arg| arg == "--all") {
        if args.len() != 1 {
            anyhow::bail!("--all 不能与资源名称或其它参数同时使用");
        }
        return Ok(Some(CliOptions { selection: FetchSelection::All, verbose }));
    }

    if let Some(option) = args.iter().find(|arg| arg.starts_with('-')) {
        anyhow::bail!("未知参数: {}", option);
    }

    if args.is_empty() {
        Ok(Some(CliOptions { selection: FetchSelection::Default, verbose }))
    } else {
        Ok(Some(CliOptions {
            selection: FetchSelection::Names(args.into_iter().collect::<BTreeSet<_>>()),
            verbose,
        }))
    }
}

fn print_help() {
    println!(
        "用法:\n  fetch_res [--verbose]                 处理默认资源\n  fetch_res <name>... [--verbose]       只处理指定资源\n  fetch_res --all [--verbose]           处理全部资源\n  fetch_res --help                      显示帮助"
    );
}
