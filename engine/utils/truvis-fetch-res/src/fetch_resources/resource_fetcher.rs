use crate::fetch_resources::resource_item::{ResourceConfig, ResourceItem, ResourceType};
use anyhow::Context;
use log::{debug, info, warn};
use std::env;
use std::fs::File;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;
use std::{fs, io};

const HTTP_CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const HTTP_DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const HTTP_DOWNLOAD_MAX_ATTEMPTS: usize = 3;
const GIT_NETWORK_MAX_ATTEMPTS: usize = 3;
const GIT_LOW_SPEED_LIMIT_BYTES: &str = "1024";
const GIT_LOW_SPEED_TIME_SECONDS: &str = "300";

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
        let config = ResourceConfig::from_file(config_path)?;
        let mut failures = Vec::new();

        for item in &config.resources {
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

    /// 下载单个资源
    pub fn fetch_resource(&self, item: &ResourceItem) -> anyhow::Result<()> {
        info!("===============================");
        info!("开始处理资源: {}", item.name);

        let target_path = PathBuf::from(&item.target_dir);
        let actual_target = target_path.join(&item.rename_to);

        if matches!(item.resource_type, ResourceType::Git) {
            self.process_git(item, &target_path, &actual_target)?;
            info!("资源 '{}' 处理完成", item.name);
            return Ok(());
        }

        // 从 URL 提取文件名
        let url_filename = self.extract_filename_from_url(&item.url)?;
        let temp_file_path = self.temp_dir.join(&url_filename);

        // 检查是否需要下载，并记录是否进行了下载
        let did_download = if !item.force_download && temp_file_path.exists() {
            info!("临时目录中已存在文件，跳过下载: {:?}", temp_file_path);
            false
        } else {
            info!("从 {} 下载资源...", item.url);
            let data = self.download_file(&item.url).with_context(|| format!("下载 {} 失败", item.name))?;

            info!("下载完成，大小: {} bytes", data.len());

            // 写入临时文件
            fs::write(&temp_file_path, &data).with_context(|| format!("写入临时文件失败: {:?}", temp_file_path))?;
            true
        };

        // 决定是否需要提取
        let should_extract = if did_download {
            // 如果进行了下载，必须提取（先删除已存在的目标）
            if actual_target.exists() {
                self.remove_resource_target(&target_path, &actual_target, "下载了新文件")?;
            }
            true
        } else if item.force_overwrite {
            // 如果强制覆盖，删除并提取
            if actual_target.exists() {
                self.remove_resource_target(&target_path, &actual_target, "强制覆盖")?;
            }
            true
        } else {
            // 否则检查目标是否存在
            !actual_target.exists()
        };

        if should_extract {
            match item.resource_type {
                ResourceType::File => {
                    self.process_file(&temp_file_path, &target_path, &item.rename_to)?;
                }
                ResourceType::Zip => {
                    self.process_zip(&temp_file_path, &target_path, &item.rename_to)?;
                }
                ResourceType::Git => unreachable!("git resource 已在下载分支前处理"),
            }
        } else {
            info!("资源已存在，跳过提取: {:?}", actual_target);
        }

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

        fs::create_dir_all(target_dir).with_context(|| format!("创建目标目录失败: {:?}", target_dir))?;

        let force_reclone = item.force_download || item.force_overwrite;
        if force_reclone && actual_target.exists() {
            self.remove_resource_target(target_dir, actual_target, "强制刷新 git resource")?;
        }

        if actual_target.exists() && !self.is_git_checkout(actual_target) {
            // zip archive 不带 `.git`，也无法恢复 submodule 的固定提交；因此旧产物要整体替换为真正的 checkout。
            self.remove_resource_target(target_dir, actual_target, "目标不是 git checkout")?;
        }

        if actual_target.exists() {
            info!("更新 Git 仓库: {:?}", actual_target);
            self.sync_existing_git_checkout(item, actual_target, git_ref)?;
        } else {
            info!("克隆 Git 仓库: {} -> {:?}", item.url, actual_target);
            self.clone_git_checkout(item, target_dir, actual_target, git_ref)?;
        }

        if item.recursive_submodules {
            info!("同步并更新 Git submodule: {:?}", actual_target);
            self.run_git_command(actual_target, &["submodule", "sync", "--recursive"])?;
            self.run_git_network_command(actual_target, &["submodule", "update", "--init", "--recursive", "--force"])?;
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
    ) -> anyhow::Result<()> {
        let actual_target_arg = actual_target.to_string_lossy().to_string();
        let clone_args = [
            "clone",
            "--origin",
            "origin",
            item.url.as_str(),
            actual_target_arg.as_str(),
        ];

        for attempt in 1..=GIT_NETWORK_MAX_ATTEMPTS {
            if actual_target.exists() {
                self.remove_resource_target(target_dir, actual_target, "清理上一次失败的 git clone 目标")?;
            }

            match self.run_git_command_once(target_dir, &clone_args) {
                Ok(()) => {
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

    fn sync_existing_git_checkout(
        &self,
        item: &ResourceItem,
        actual_target: &Path,
        git_ref: &str,
    ) -> anyhow::Result<()> {
        if self.run_git_command(actual_target, &["remote", "set-url", "origin", item.url.as_str()]).is_err() {
            self.run_git_command(actual_target, &["remote", "add", "origin", item.url.as_str()])?;
        }
        self.checkout_git_ref(item, actual_target, git_ref)
    }

    fn checkout_git_ref(&self, item: &ResourceItem, actual_target: &Path, git_ref: &str) -> anyhow::Result<()> {
        self.run_git_network_command(actual_target, &["fetch", "--tags", "--prune", "origin"])
            .with_context(|| format!("更新 git resource '{}' 的远端引用失败", item.name))?;
        self.run_git_command(actual_target, &["checkout", "--force", git_ref])
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
            Ok(())
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
    /// 从 URL 提取文件名
    fn extract_filename_from_url(&self, url: &str) -> anyhow::Result<String> {
        url.split('/')
            .next_back()
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow::anyhow!("无法从 URL 提取文件名: {}", url))
    }

    /// 下载文件到内存
    fn download_file(&self, url: &str) -> anyhow::Result<Vec<u8>> {
        for attempt in 1..=HTTP_DOWNLOAD_MAX_ATTEMPTS {
            match self.download_file_once(url) {
                Ok(data) => return Ok(data),
                Err(e) if attempt < HTTP_DOWNLOAD_MAX_ATTEMPTS => {
                    warn!("HTTP 下载失败，准备重试 ({}/{}): {:?}", attempt, HTTP_DOWNLOAD_MAX_ATTEMPTS, e);
                    Self::sleep_before_retry(attempt);
                }
                Err(e) => return Err(e),
            }
        }

        unreachable!("HTTP retry loop 应该已经返回")
    }

    fn download_file_once(&self, url: &str) -> anyhow::Result<Vec<u8>> {
        debug!("请求 URL: {}", url);

        let response = self.client.get(url).send().context("发送 HTTP 请求失败")?;

        if !response.status().is_success() {
            anyhow::bail!("HTTP 请求失败: {}", response.status());
        }

        let mut data = Vec::new();
        let mut reader = response;
        reader.read_to_end(&mut data).context("读取响应数据失败")?;

        Ok(data)
    }
}

// 目标删除
impl GitHubResourceFetcher {
    /// 删除配置声明的资源目标。
    ///
    /// 资源下载工具会替换旧 zip 产物或强制刷新的 checkout，因此删除前必须确认目标位于
    /// `target_dir` 之下，避免错误配置把递归删除扩散到资源目录之外。
    fn remove_resource_target(&self, target_dir: &Path, actual_target: &Path, reason: &str) -> anyhow::Result<()> {
        let target_dir_abs =
            target_dir.canonicalize().with_context(|| format!("解析目标目录失败: {:?}", target_dir))?;
        let actual_target_abs =
            actual_target.canonicalize().with_context(|| format!("解析待删除目标失败: {:?}", actual_target))?;

        if actual_target_abs == target_dir_abs || !actual_target_abs.starts_with(&target_dir_abs) {
            anyhow::bail!(
                "拒绝删除不在目标目录内的资源路径: target_dir={:?}, actual_target={:?}",
                target_dir_abs,
                actual_target_abs
            );
        }

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
    /// 处理普通文件：复制到目标目录，重命名
    fn process_file(&self, temp_file: &Path, target_dir: &Path, rename_to: &str) -> anyhow::Result<()> {
        // 确保目标目录存在
        fs::create_dir_all(target_dir).with_context(|| format!("创建目标目录失败: {:?}", target_dir))?;

        let target_path = target_dir.join(rename_to);

        info!("复制文件: {:?} -> {:?}", temp_file, target_path);
        fs::copy(temp_file, &target_path)
            .with_context(|| format!("复制文件失败: {:?} -> {:?}", temp_file, target_path))?;

        Ok(())
    }

    /// 处理 Zip 文件：解压到目标目录，重命名顶级目录
    fn process_zip(&self, temp_file: &Path, target_dir: &Path, rename_to: &str) -> anyhow::Result<()> {
        let zip_data = fs::read(temp_file).with_context(|| format!("读取 zip 文件失败: {:?}", temp_file))?;

        self.extract_zip(&zip_data, target_dir, rename_to)
    }

    /// 解压 zip 文件到目标目录
    fn extract_zip(&self, zip_data: &[u8], target_dir: &Path, rename_to: &str) -> anyhow::Result<()> {
        let cursor = Cursor::new(zip_data);
        let mut archive = zip::ZipArchive::new(cursor).context("打开 zip archive 失败")?;

        info!("解压 {} 个文件到 {:?}", archive.len(), target_dir);

        // 创建目标目录
        fs::create_dir_all(target_dir).with_context(|| format!("创建目标目录失败: {:?}", target_dir))?;

        for i in 0..archive.len() {
            let mut file = archive.by_index(i).with_context(|| format!("读取 zip 条目 {} 失败", i))?;

            let file_path = file.name();

            // 跳过空路径
            if file_path.is_empty() {
                continue;
            }

            // 处理路径
            let out_path = target_dir.join(rename_to).join(file_path);
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
