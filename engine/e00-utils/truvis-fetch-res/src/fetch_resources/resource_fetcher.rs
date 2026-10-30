use std::collections::BTreeSet;
use std::env;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::thread;
use std::time::Duration;
use std::{fs, io};

use anyhow::Context;
use log::{debug, info, warn};
use sha2::{Digest, Sha256};

use crate::fetch_resources::resource_item::{ResourceConfig, ResourceItem, ResourceType};

const HTTP_CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const HTTP_DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const HTTP_DOWNLOAD_MAX_ATTEMPTS: usize = 3;
const GIT_NETWORK_MAX_ATTEMPTS: usize = 3;
const GIT_LOW_SPEED_LIMIT_BYTES: &str = "1024";
const GIT_LOW_SPEED_TIME_SECONDS: &str = "300";

/// 批量资源处理策略。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchSelection {
    /// 只处理配置中默认参与下载的资源。
    Default,
    /// 处理配置中的全部资源。
    All,
    /// 只处理显式指定名称的资源。
    Names(BTreeSet<String>),
}

/// 资源下载器
pub struct GitHubResourceFetcher {
    client: reqwest::blocking::Client,
    /// 临时目录路径，默认为 "temp"
    temp_dir: PathBuf,
}

// 创建与初始化
impl GitHubResourceFetcher {
    /// 使用自定义临时目录创建下载器
    pub fn with_temp_dir<P: AsRef<Path>>(temp_dir: P) -> anyhow::Result<Self> {
        let client = reqwest::blocking::Client::builder()
            .user_agent("Truvis-Resource-Fetcher/1.0")
            .connect_timeout(HTTP_CONNECT_TIMEOUT)
            .timeout(HTTP_DOWNLOAD_TIMEOUT)
            .build()
            .context("创建 HTTP 客户端失败")?;

        let temp_dir = PathBuf::from(temp_dir.as_ref());

        // 确保临时目录存在
        if !temp_dir.exists() {
            fs::create_dir_all(&temp_dir).with_context(|| format!("创建临时目录失败: {:?}", temp_dir))?;
        }

        Self::log_proxy_environment();

        Ok(Self { client, temp_dir })
    }

    fn log_proxy_environment() {
        let proxy_names = [
            "HTTP_PROXY",
            "HTTPS_PROXY",
            "ALL_PROXY",
            "NO_PROXY",
            "http_proxy",
            "https_proxy",
            "all_proxy",
            "no_proxy",
        ];
        let present_names = proxy_names
            .iter()
            .copied()
            .filter(|name| env::var(name).is_ok_and(|value| !value.is_empty()))
            .collect::<Vec<_>>();

        if present_names.is_empty() {
            info!("未检测到 HTTP proxy 环境变量；HTTP 下载和 git 将直接访问远端");
        } else {
            // 只记录变量名，不记录代理地址，避免把带账号密码的 proxy URL 写入日志。
            info!("检测到 HTTP proxy 环境变量: {}", present_names.join(", "));
        }
    }
}

// 工具函数
impl GitHubResourceFetcher {
    /// 从配置文件批量下载资源
    pub fn fetch_from_config<P: AsRef<Path>>(&self, config_path: P) -> anyhow::Result<()> {
        self.fetch_from_config_with_selection(config_path, FetchSelection::Default)
    }

    /// 按选择策略从配置文件批量下载资源。
    pub fn fetch_from_config_with_selection<P: AsRef<Path>>(
        &self,
        config_path: P,
        selection: FetchSelection,
    ) -> anyhow::Result<()> {
        let config = ResourceConfig::from_file(config_path)?;
        Self::validate_selection(&config, &selection)?;
        let mut failures = Vec::new();

        for item in &config.resources {
            if !Self::should_fetch(item, &selection) {
                if matches!(&selection, FetchSelection::Default) {
                    info!("跳过非默认资源 '{}'; 如需下载请显式指定该名称", item.name);
                }
                continue;
            }

            if let Err(e) = self.fetch_resource(item) {
                warn!("下载资源 '{}' 失败: {:?}", item.name, e);
                failures.push(item.name.clone());
            }
        }

        if !failures.is_empty() {
            anyhow::bail!("部分资源下载失败: {}", failures.join(", "));
        }

        Ok(())
    }

    fn validate_selection(config: &ResourceConfig, selection: &FetchSelection) -> anyhow::Result<()> {
        let FetchSelection::Names(names) = selection else {
            return Ok(());
        };

        let missing = names
            .iter()
            .filter(|name| !config.resources.iter().any(|item| item.name == **name))
            .cloned()
            .collect::<Vec<_>>();

        if !missing.is_empty() {
            anyhow::bail!("未找到资源: {}", missing.join(", "));
        }

        Ok(())
    }

    fn should_fetch(item: &ResourceItem, selection: &FetchSelection) -> bool {
        match selection {
            FetchSelection::Default => item.download_by_default,
            FetchSelection::All => true,
            FetchSelection::Names(names) => names.contains(&item.name),
        }
    }

    /// 下载单个资源
    pub fn fetch_resource(&self, item: &ResourceItem) -> anyhow::Result<()> {
        info!("=============================== 开始处理资源: {} ===============================", item.name);

        for name in [&item.name, &item.rename_to] {
            let relative = Self::archive_relative_path(name)?;
            anyhow::ensure!(
                relative.components().count() == 1
                    && !name.contains(['/', '\\'])
                    && !name.to_ascii_lowercase().starts_with(".truvis-"),
                "资源名称必须是普通单层名称，且不能占用 .truvis- 前缀: {name:?}"
            );
        }
        let target_path = PathBuf::from(&item.target_dir);
        fs::create_dir_all(&target_path)?;
        let target_path = target_path.canonicalize()?;
        let actual_target = target_path.join(&item.rename_to);
        self.recover_target(&target_path, &actual_target)?;

        let target_key = actual_target.to_string_lossy().into_owned();
        #[cfg(windows)]
        let target_key = target_key.to_lowercase();
        let receipt_dir = self.temp_dir.join("published");
        fs::create_dir_all(&receipt_dir)?;
        let receipt = receipt_dir.join(format!("{:x}", Sha256::digest(target_key.as_bytes())));
        self.recover_target(&receipt_dir, &receipt)?;
        if matches!(item.resource_type, ResourceType::Git) {
            // HTTP → Git → HTTP 切换时，不能保留旧 HTTP 完成记录。
            if receipt.exists() {
                self.remove_resource_target(&receipt_dir, &receipt, "撤销旧 HTTP 资源完成记录")?;
            }
            self.process_git(item, &target_path, &actual_target)?;
            info!("资源 '{}' 处理完成", item.name);
            return Ok(());
        }

        let url_key = format!("{:x}", Sha256::digest(item.url.as_bytes()));
        let identity = format!("http-v2:{url_key}:{:?}:{target_key}", item.resource_type);
        let target_type_matches = match item.resource_type {
            ResourceType::File => actual_target.is_file(),
            ResourceType::Zip | ResourceType::SevenZip => actual_target.is_dir(),
            ResourceType::Git => unreachable!(),
        };
        // receipt 只认证工具曾发布的来源/类型/目标；手工改动目标需要 force，不能靠缓存存在推断完成。
        if !item.force_download && !item.force_overwrite && target_type_matches
            && fs::read_to_string(&receipt).is_ok_and(|saved| saved.split_once('\n').is_some_and(|(source, digest)| source == identity && digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())))
        {
            info!("已发布相同资源，跳过缓存访问和提取: {:?}", actual_target);
            return Ok(());
        }
        let (payload, digest) = self.cached_download(item, &url_key)?;

        // 在更改目标前撤销完成记录，避免进程中止后用旧记录认证新目标。
        if receipt.exists() {
            self.remove_resource_target(&receipt_dir, &receipt, "撤销旧资源完成记录")?;
        }
        if matches!(item.resource_type, ResourceType::Zip | ResourceType::SevenZip) {
            self.process_archive(item, &payload, &target_path)?;
        } else {
            self.process_file(&payload, &target_path, &item.rename_to)?;
        }
        let staging = self.prepare_staging(&receipt_dir, &receipt)?;
        fs::write(&staging, format!("{identity}\n{digest}"))?;
        self.publish_prepared(&receipt_dir, &staging, &receipt)?;

        info!("资源 '{}' 处理完成", item.name);
        Ok(())
    }
}

// Git checkout
impl GitHubResourceFetcher {
    /// 处理 Git 资源：目标目录由 fetcher 管理，并保留 `.git` 与 submodule 元数据供后续源码分析。
    fn process_git(&self, item: &ResourceItem, target_dir: &Path, actual_target: &Path) -> anyhow::Result<()> {
        self.validate_git_url(&item.url)?;

        let git_ref =
            item.git_ref.as_deref().ok_or_else(|| anyhow::anyhow!("git resource '{}' 缺少 git_ref", item.name))?;

        let force_reclone = item.force_download || item.force_overwrite;
        let reuse_checkout = self.is_git_checkout(actual_target) && !force_reclone;
        anyhow::ensure!(!git_ref.is_empty() && !git_ref.starts_with('-'), "非法 git_ref: {git_ref:?}");
        anyhow::ensure!(!actual_target.exists() || reuse_checkout || force_reclone, "资源目标不是 Git checkout，拒绝覆盖: {actual_target:?}");
        if reuse_checkout {
            self.ensure_clean_checkout(actual_target)?;
            let origin = self.run_git_output(actual_target, &["remote", "get-url", "origin"])?;
            anyhow::ensure!(String::from_utf8_lossy(&origin.stdout).trim() == item.url, "Git 来源与配置不一致，拒绝复用: {actual_target:?}");
            let fixed_commit = matches!(git_ref.len(), 40 | 64) && git_ref.bytes().all(|byte| byte.is_ascii_hexdigit());
            let tag = if git_ref.starts_with("refs/tags/") { git_ref.to_string() } else { format!("refs/tags/{git_ref}") };
            let local_fixed = self.resolve_git_commit(actual_target, if fixed_commit { git_ref } else { &tag });
            let target_commit = if let Some(commit) = local_fixed {
                commit
            } else {
                self.run_git_network_command(actual_target, &["fetch", "--prune", "origin", git_ref])?;
                self.resolve_git_commit(actual_target, "FETCH_HEAD").ok_or_else(|| anyhow::anyhow!("fetched ref has no commit"))?
            };
            if self.resolve_git_commit(actual_target, "HEAD").as_ref() == Some(&target_commit) {
                if !item.recursive_submodules || self.submodules_match(actual_target)? {
                    info!("Git 资源已匹配，跳过 checkout: {}", item.name);
                    return Ok(());
                }
            } else {
                self.run_git_command(actual_target, &["checkout", "--detach", &target_commit])?;
            }
            self.update_submodules(item, actual_target)?;
            return Ok(());
        }

        let staging = self.prepare_staging(target_dir, actual_target)?;
        let source = &item.url;
        self.clone_git_checkout(item, target_dir, &staging, git_ref, source)?;

        self.update_submodules(item, &staging)?;

        self.publish_prepared(target_dir, &staging, actual_target)
    }

    /// 已有 checkout 原地更新前保护本地状态；gitlink 版本漂移由完整匹配检查修复。
    fn ensure_clean_checkout(&self, checkout: &Path) -> anyhow::Result<()> {
            let status = self.run_git_output(
                checkout,
                &[
                    "status",
                    "--porcelain",
                    "--untracked-files=all",
                    "--ignored",
                    "--ignore-submodules=all",
                ],
            )?;
            anyhow::ensure!(
                status.stdout.is_empty(),
                "Git 资源存在本地改动或额外文件，拒绝更新: {checkout:?}；确认丢弃后使用 force_overwrite"
            );
            let stash = self.run_git_output(checkout, &["stash", "list", "--format=%H"])?;
            anyhow::ensure!(stash.stdout.is_empty(), "Git 资源存在 stash，拒绝更新: {checkout:?}");
            // submodule 的 ignored 文件不会出现在父仓库 status 中，独立检查已初始化的子仓库。
            let status = self.run_git_output(
                checkout,
                &[
                    "submodule",
                    "foreach",
                    "--quiet",
                    "--recursive",
                    "git status --porcelain --untracked-files=all --ignored --ignore-submodules=none && git stash list --format=%H",
                ],
            )?;
            anyhow::ensure!(status.stdout.is_empty(), "Git submodule 存在本地文件或 stash，拒绝更新: {checkout:?}");
        Ok(())
    }

    fn resolve_git_commit(&self, checkout: &Path, reference: &str) -> Option<String> {
        self.run_git_output(checkout, &["rev-parse", "--verify", "--quiet", "--end-of-options", &format!("{reference}^{{commit}}")])
            .ok().map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    fn submodules_match(&self, checkout: &Path) -> anyhow::Result<bool> {
        let result = self.run_git_output(checkout, &["submodule", "status", "--recursive"])?;
        Ok(String::from_utf8_lossy(&result.stdout).lines().all(|line| line.starts_with(' ')))
    }

    fn update_submodules(&self, item: &ResourceItem, checkout: &Path) -> anyhow::Result<()> {
        if item.recursive_submodules {
            self.run_git_command(checkout, &["submodule", "sync", "--recursive"])?;
            self.run_git_network_command(checkout, &["submodule", "update", "--init", "--recursive"])?;
            anyhow::ensure!(self.submodules_match(checkout)?, "Git submodule 更新后未完整匹配: {checkout:?}");
        }
        Ok(())
    }

    fn validate_git_url(&self, url: &str) -> anyhow::Result<()> {
        if url.starts_with("http://") || url.starts_with("https://") {
            Ok(())
        } else {
            anyhow::bail!("git resource 只允许 http(s) URL，不允许使用 git:// 或 SSH: {}", url);
        }
    }

    fn is_git_checkout(&self, path: &Path) -> bool {
        path.join(".git").exists()
    }

    fn clone_git_checkout(
        &self,
        item: &ResourceItem,
        target_dir: &Path,
        actual_target: &Path,
        git_ref: &str,
        source: &str,
    ) -> anyhow::Result<()> {
        // clone 在目标父目录执行；相对名称避免把 Windows verbatim 路径传给 Git 的参数解析器。
        let actual_target_arg = actual_target.file_name().unwrap().to_string_lossy();
        let clone_args = [
            "clone",
            // staging 必须拥有独立对象库，发布后删除 previous 不得使它失效。
            "--no-hardlinks",
            "--dissociate",
            "--no-checkout",
            "--origin",
            "origin",
            source,
            &actual_target_arg,
        ];

        for attempt in 1..=GIT_NETWORK_MAX_ATTEMPTS {
            if actual_target.exists() {
                self.remove_resource_target(target_dir, actual_target, "清理上一次失败的 git clone 目标")?;
            }

            match self.run_git_command_once(target_dir, &clone_args) {
                Ok(()) => {
                    self.run_git_command(actual_target, &["remote", "set-url", "origin", &item.url])?;
                    self.checkout_git_ref(item, actual_target, git_ref)?;
                    return Ok(());
                }
                Err(e) if attempt < GIT_NETWORK_MAX_ATTEMPTS => {
                    warn!("git clone 失败，准备重试 ({}/{}): {:?}", attempt, GIT_NETWORK_MAX_ATTEMPTS, e);
                    Self::sleep_before_retry(attempt);
                }
                Err(e) => return Err(e).with_context(|| format!("克隆 git resource '{}' 失败", item.name)),
            }
        }

        unreachable!("git clone retry loop 应该已经返回")
    }

    fn checkout_git_ref(&self, item: &ResourceItem, actual_target: &Path, git_ref: &str) -> anyhow::Result<()> {
        // checkout 本次获取的 ref，避免同名本地 branch 停留在旧提交；tag/commit 同样使用精确结果。
        self.run_git_network_command(actual_target, &["fetch", "--prune", "origin", git_ref])
            .with_context(|| format!("更新 git resource '{}' 的远端引用失败", item.name))?;
        self.run_git_command(actual_target, &["checkout", "--force", "--detach", "FETCH_HEAD"])
            .with_context(|| format!("checkout git resource '{}' 到 '{}' 失败", item.name, git_ref))?;
        Ok(())
    }

    fn run_git_network_command(&self, current_dir: &Path, args: &[&str]) -> anyhow::Result<()> {
        for attempt in 1..=GIT_NETWORK_MAX_ATTEMPTS {
            match self.run_git_command_once(current_dir, args) {
                Ok(()) => return Ok(()),
                Err(e) if attempt < GIT_NETWORK_MAX_ATTEMPTS => {
                    warn!(
                        "git 网络命令失败，准备重试 ({}/{}): git {}: {:?}",
                        attempt,
                        GIT_NETWORK_MAX_ATTEMPTS,
                        args.join(" "),
                        e
                    );
                    Self::sleep_before_retry(attempt);
                }
                Err(e) => return Err(e),
            }
        }

        unreachable!("git network retry loop 应该已经返回")
    }

    fn run_git_command(&self, current_dir: &Path, args: &[&str]) -> anyhow::Result<()> {
        self.run_git_command_once(current_dir, args)
    }

    fn run_git_command_once(&self, current_dir: &Path, args: &[&str]) -> anyhow::Result<()> {
        self.run_git_output(current_dir, args).map(|_| ())
    }

    fn run_git_output(&self, current_dir: &Path, args: &[&str]) -> anyhow::Result<Output> {
        debug!("执行 git {:?}: git {}", current_dir, args.join(" "));

        let output = Command::new("git")
            .args(args)
            .current_dir(current_dir)
            // fetch_res 是批处理工具，遇到需要认证的代理或远端时必须失败并给出错误，而不是卡在交互式输入。
            .env("GIT_TERMINAL_PROMPT", "0")
            // 对大 submodule/asset 仓库给足时间，但低速网络长时间无进展时要让 git 退出，交给上层重试。
            .env("GIT_HTTP_LOW_SPEED_LIMIT", GIT_LOW_SPEED_LIMIT_BYTES)
            .env("GIT_HTTP_LOW_SPEED_TIME", GIT_LOW_SPEED_TIME_SECONDS)
            .output()
            .with_context(|| format!("启动 git 命令失败: git {}", args.join(" ")))?;

        if output.status.success() {
            Ok(output)
        } else {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!(
                "git 命令失败: git {}\n当前目录: {:?}\nstdout:\n{}\nstderr:\n{}",
                args.join(" "),
                current_dir,
                stdout.trim(),
                stderr.trim()
            );
        }
    }

    fn sleep_before_retry(attempt: usize) {
        thread::sleep(Duration::from_secs((attempt as u64) * 2));
    }
}

// 下载
impl GitHubResourceFetcher {
    /// 缓存以完整 URL 区分来源；摘要只证明本地完整性，不认证上游内容。
    fn cached_download(&self, item: &ResourceItem, key: &str) -> anyhow::Result<(PathBuf, String)> {
        let root = self.temp_dir.join("http");
        fs::create_dir_all(&root)?;
        let cache = root.join(key);
        self.recover_target(&root, &cache)?;
        let payload = cache.join("payload");
        if !item.force_download && payload.is_file() {
            if let Ok(expected) = fs::read_to_string(cache.join("sha256")) {
                let actual = Self::content_digest(&payload)?;
                if actual == expected {
                    return Ok((payload, actual));
                }
            }
        }
        let staging = self.prepare_staging(&root, &cache)?;
        fs::create_dir(&staging)?;
        let digest = self.download_file(&item.url, &staging.join("payload"))?;
        fs::write(staging.join("sha256"), &digest)?;
        self.publish_prepared(&root, &staging, &cache)?;
        Ok((payload, digest))
    }

    fn content_digest(path: &Path) -> anyhow::Result<String> {
        let mut input = File::open(path)?;
        let mut hash = Sha256::new();
        let mut buffer = [0; 64 * 1024];
        loop {
            let size = input.read(&mut buffer)?;
            if size == 0 {
                break;
            }
            hash.update(&buffer[..size]);
        }
        Ok(format!("{:x}", hash.finalize()))
    }

    /// 每次重试重建 staging payload；只有完整响应才会产生可发布的摘要。
    fn download_file(&self, url: &str, output: &Path) -> anyhow::Result<String> {
        for attempt in 1..=HTTP_DOWNLOAD_MAX_ATTEMPTS {
            match self.download_file_once(url, output) {
                Ok(digest) => return Ok(digest),
                Err(e) if attempt < HTTP_DOWNLOAD_MAX_ATTEMPTS => {
                    warn!("HTTP 下载失败，准备重试 ({}/{}): {:?}", attempt, HTTP_DOWNLOAD_MAX_ATTEMPTS, e);
                    Self::sleep_before_retry(attempt);
                }
                Err(e) => return Err(e),
            }
        }
        unreachable!("HTTP retry loop 应该已经返回")
    }

    fn download_file_once(&self, url: &str, output: &Path) -> anyhow::Result<String> {
        let mut response = self.client.get(url).send().context("发送 HTTP 请求失败")?;
        anyhow::ensure!(response.status().is_success(), "HTTP 请求失败: {}", response.status());
        let mut file = File::create(output)?;
        let mut hash = Sha256::new();
        let mut buffer = [0; 64 * 1024];
        loop {
            let size = response.read(&mut buffer).context("读取响应数据失败")?;
            if size == 0 {
                break;
            }
            file.write_all(&buffer[..size])?;
            hash.update(&buffer[..size]);
        }
        file.flush()?;
        Ok(format!("{:x}", hash.finalize()))
    }
}

// 目标删除
impl GitHubResourceFetcher {
    /// 发布槽与目标同父目录，保证 rename 不跨卷。调用方独占父目录中的这些保留名称。
    fn publication_slot(target: &Path, suffix: &str) -> PathBuf {
        target.with_file_name(format!(".truvis-{}.{suffix}", target.file_name().unwrap().to_string_lossy()))
    }

    /// 移动和删除前检查真实边界；不接管 symlink/junction，包括已断开的链接。
    fn validate_target(target_dir: &Path, target: &Path) -> anyhow::Result<bool> {
        let metadata = match fs::symlink_metadata(target) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error.into()),
        };
        anyhow::ensure!(!metadata.file_type().is_symlink(), "资源目标不能是链接: {target:?}");
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            anyhow::ensure!(metadata.file_attributes() & 0x400 == 0, "资源目标不能是 reparse point: {target:?}");
        }
        let root = target_dir.canonicalize()?;
        let actual = target.canonicalize()?;
        anyhow::ensure!(actual.parent() == Some(root.as_path()), "资源目标必须直接位于配置父目录内: {target:?}");
        Ok(true)
    }

    /// previous 存在但目标缺失表示发布中断，先恢复旧目标；两者并存表示新目标已发布。
    /// 只保证进程失败后的可重试性，不保证断电持久性或多个资源的事务性。
    fn recover_target(&self, root: &Path, target: &Path) -> anyhow::Result<()> {
        let exists = Self::validate_target(root, target)?;
        let previous = Self::publication_slot(target, "previous");
        if Self::validate_target(root, &previous)? {
            if exists {
                self.remove_resource_target(root, &previous, "清理已发布资源的旧版本")?;
            } else {
                fs::rename(&previous, target).context("恢复资源旧版本失败")?;
            }
        }
        Ok(())
    }

    fn prepare_staging(&self, root: &Path, target: &Path) -> anyhow::Result<PathBuf> {
        self.recover_target(root, target)?;
        let staging = Self::publication_slot(target, "staging");
        if Self::validate_target(root, &staging)? {
            self.remove_resource_target(root, &staging, "清理未完成的资源准备")?;
        }
        Ok(staging)
    }

    /// 仅接受已经完整准备的 staging。新目标 rename 失败时恢复旧版本；恢复失败保留 previous。
    fn publish_prepared(&self, root: &Path, staging: &Path, target: &Path) -> anyhow::Result<()> {
        anyhow::ensure!(Self::validate_target(root, staging)?, "资源 staging 缺失: {staging:?}");
        let previous = Self::publication_slot(target, "previous");
        anyhow::ensure!(!Self::validate_target(root, &previous)?, "发布前必须恢复 previous: {previous:?}");
        let had_old = Self::validate_target(root, target)?;
        if had_old {
            fs::rename(target, &previous).context("暂存旧资源失败，目标未替换")?;
        }
        if let Err(error) = fs::rename(staging, target) {
            if had_old {
                fs::rename(&previous, target)
                    .with_context(|| format!("发布失败: {error}; 恢复旧版本也失败，旧资源保留在 {previous:?}"))?;
            }
            return Err(error).context("发布资源失败，旧目标已恢复");
        }
        if had_old {
            self.remove_resource_target(root, &previous, "清理已替换的资源")?;
        }
        Ok(())
    }

    /// 删除配置声明的资源目标。
    ///
    /// 资源下载工具会替换旧 zip 产物或强制刷新的 checkout，因此删除前必须确认目标位于
    /// `target_dir` 之下，避免错误配置把递归删除扩散到资源目录之外。
    fn remove_resource_target(&self, target_dir: &Path, actual_target: &Path, reason: &str) -> anyhow::Result<()> {
        anyhow::ensure!(Self::validate_target(target_dir, actual_target)?, "待删除资源不存在: {actual_target:?}");
        info!("{}，删除现有目标: {:?}", reason, actual_target);
        if actual_target.is_dir() {
            fs::remove_dir_all(actual_target).with_context(|| format!("删除现有目录失败: {:?}", actual_target))?;
        } else {
            fs::remove_file(actual_target).with_context(|| format!("删除现有文件失败: {:?}", actual_target))?;
        }

        Ok(())
    }
}

// 提取
impl GitHubResourceFetcher {
    /// ZIP/7z 只在完整解压成功后发布；失败的 staging 不得成为下次执行的已完成目标。
    /// staging 与目标目录由下载器独占，和已有缓存一样不支持多进程同时刷新同一资源。
    fn process_archive(&self, item: &ResourceItem, archive_path: &Path, target_dir: &Path) -> anyhow::Result<()> {
        let actual_target = target_dir.join(&item.rename_to);
        let staging = self.prepare_staging(target_dir, &actual_target)?;
        fs::create_dir(&staging).context("创建 archive staging 失败")?;

        let extraction = match item.resource_type {
            ResourceType::Zip => self.extract_zip(archive_path, &staging),
            ResourceType::SevenZip => self.extract_seven_zip(archive_path, &staging),
            _ => unreachable!("只有 ZIP/7z 进入 archive 解压流程"),
        };
        if let Err(error) = extraction {
            self.remove_resource_target(target_dir, &staging, "清理失败的 archive staging")?;
            return Err(error).with_context(|| {
                format!("解压资源 '{}' 失败；若缓存损坏，请设置 force_download = true 后重试", item.name)
            });
        }

        self.publish_prepared(target_dir, &staging, &actual_target)
    }

    /// 先核验所有条目，再从文件流解压。只创建普通文件和目录，不恢复 archive 中的链接或属性。
    fn extract_seven_zip(&self, archive_path: &Path, staging: &Path) -> anyhow::Result<()> {
        let mut archive = sevenz_rust2::ArchiveReader::open(archive_path, sevenz_rust2::Password::empty())?;
        for entry in &archive.archive().files {
            Self::archive_relative_path(&entry.name)?;
            anyhow::ensure!(!entry.is_anti_item, "7z 不支持删除条目: {}", entry.name);
        }
        info!("解压 {} 个 7z 条目到 {:?}", archive.archive().files.len(), staging);
        archive.for_each_entries(|entry, reader| {
            let relative = Self::archive_relative_path(&entry.name)
                .map_err(|error| sevenz_rust2::Error::Other(error.to_string().into()))?;
            let output = staging.join(relative);
            if entry.is_directory {
                fs::create_dir_all(&output)?;
            } else {
                if let Some(parent) = output.parent() {
                    fs::create_dir_all(parent)?;
                }
                let mut file = File::create(&output)?;
                io::copy(reader, &mut file)?;
            }
            Ok(true)
        })?;
        Ok(())
    }

    /// 按 Windows 文件名语义校验，即使在其它平台也拒绝盘符、ADS、设备名和路径归一化歧义。
    /// 同时识别两种分隔符，防止仅依赖当前平台 Path 解析而漏掉另一平台的越界路径。
    fn archive_relative_path(name: &str) -> anyhow::Result<PathBuf> {
        anyhow::ensure!(!name.is_empty() && !name.starts_with(['/', '\\']), "非法 archive 路径: {name:?}");
        let mut path = PathBuf::new();
        for part in name.trim_end_matches(['/', '\\']).split(['/', '\\']) {
            let stem = part.split('.').next().unwrap_or_default().to_ascii_uppercase();
            let numbered_device = stem.strip_prefix("COM").or_else(|| stem.strip_prefix("LPT")).is_some_and(|suffix| {
                matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³")
            });
            anyhow::ensure!(
                !part.is_empty()
                    && part != "."
                    && part != ".."
                    && !part.ends_with(['.', ' '])
                    && !part.chars().any(|ch| ch.is_control() || matches!(ch, ':' | '<' | '>' | '"' | '|' | '?' | '*'))
                    && !matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                    && !numbered_device,
                "非法 archive 路径: {name:?}"
            );
            path.push(part);
        }
        Ok(path)
    }

    /// 处理普通文件：复制到目标目录，重命名
    fn process_file(&self, temp_file: &Path, target_dir: &Path, rename_to: &str) -> anyhow::Result<()> {
        let target = target_dir.join(rename_to);
        let staging = self.prepare_staging(target_dir, &target)?;
        fs::copy(temp_file, &staging).context("准备普通文件失败，旧目标保留")?;
        self.publish_prepared(target_dir, &staging, &target)
    }

    /// 先校验全部条目，再向 staging 写普通文件/目录；不恢复 archive 链接或属性。
    fn extract_zip(&self, archive_path: &Path, staging: &Path) -> anyhow::Result<()> {
        let file = File::open(archive_path).with_context(|| format!("读取 zip 文件失败: {:?}", archive_path))?;
        let mut archive = zip::ZipArchive::new(file).context("打开 zip archive 失败")?;
        for i in 0..archive.len() {
            let entry = archive.by_index(i).with_context(|| format!("读取 zip 条目 {} 失败", i))?;
            Self::archive_relative_path(entry.name())?;
        }
        info!("解压 {} 个 ZIP 条目到 {:?}", archive.len(), staging);

        for i in 0..archive.len() {
            let mut file = archive.by_index(i).with_context(|| format!("读取 zip 条目 {} 失败", i))?;

            let out_path = staging.join(Self::archive_relative_path(file.name())?);
            log::debug!("解压文件到: {:?}", out_path);

            if file.is_dir() {
                // 创建目录
                fs::create_dir_all(&out_path).with_context(|| format!("创建目录失败: {:?}", out_path))?;
            } else {
                // 确保父目录存在
                if let Some(parent) = out_path.parent() {
                    fs::create_dir_all(parent).with_context(|| format!("创建父目录失败: {:?}", parent))?;
                }

                // 写入文件
                let mut outfile = File::create(&out_path).with_context(|| format!("创建文件失败: {:?}", out_path))?;

                io::copy(&mut file, &mut outfile).with_context(|| format!("写入文件失败: {:?}", out_path))?;
            }
        }

        Ok(())
    }
}
