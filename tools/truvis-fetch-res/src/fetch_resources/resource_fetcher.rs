use crate::fetch_resources::resource_item::{ResourceConfig, ResourceItem, ResourceType};
use anyhow::Context;
use log::{debug, info, warn};
use std::fs::File;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::{fs, io};

/// 资源下载器
pub struct GitHubResourceFetcher {
    client: reqwest::blocking::Client,
    /// 临时目录路径，默认为 "temp"
    temp_dir: PathBuf,
}

// new & init
impl GitHubResourceFetcher {
    /// 使用自定义临时目录创建下载器
    pub fn with_temp_dir<P: AsRef<Path>>(temp_dir: P) -> anyhow::Result<Self> {
        let client = reqwest::blocking::Client::builder()
            .user_agent("Truvis-Resource-Fetcher/1.0")
            .build()
            .context("创建 HTTP 客户端失败")?;

        let temp_dir = PathBuf::from(temp_dir.as_ref());

        // 确保临时目录存在
        if !temp_dir.exists() {
            fs::create_dir_all(&temp_dir).with_context(|| format!("创建临时目录失败: {:?}", temp_dir))?;
        }

        Ok(Self { client, temp_dir })
    }
}

// tools
impl GitHubResourceFetcher {
    /// 从配置文件批量下载资源
    pub fn fetch_from_config<P: AsRef<Path>>(&self, config_path: P) -> anyhow::Result<()> {
        let config = ResourceConfig::from_file(config_path)?;

        for item in &config.resources {
            if let Err(e) = self.fetch_resource(item) {
                warn!("下载资源 '{}' 失败: {:?}", item.name, e);
            }
        }

        Ok(())
    }

    /// 下载单个资源
    pub fn fetch_resource(&self, item: &ResourceItem) -> anyhow::Result<()> {
        info!("===============================");
        info!("开始处理资源: {}", item.name);

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

        // 根据资源类型处理
        let target_path = PathBuf::from(&item.target_dir);
        let actual_target = target_path.join(&item.rename_to);

        // 决定是否需要提取
        let should_extract = if did_download {
            // 如果进行了下载，必须提取（先删除已存在的目标）
            if actual_target.exists() {
                info!("下载了新文件，删除旧的目标: {:?}", actual_target);
                if actual_target.is_dir() {
                    fs::remove_dir_all(&actual_target)
                        .with_context(|| format!("删除现有目录失败: {:?}", actual_target))?;
                } else {
                    fs::remove_file(&actual_target)
                        .with_context(|| format!("删除现有文件失败: {:?}", actual_target))?;
                }
            }
            true
        } else if item.force_overwrite {
            // 如果强制覆盖，删除并提取
            if actual_target.exists() {
                info!("强制覆盖，删除现有目录/文件: {:?}", actual_target);
                if actual_target.is_dir() {
                    fs::remove_dir_all(&actual_target)
                        .with_context(|| format!("删除现有目录失败: {:?}", actual_target))?;
                } else {
                    fs::remove_file(&actual_target)
                        .with_context(|| format!("删除现有文件失败: {:?}", actual_target))?;
                }
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
            }
        } else {
            info!("资源已存在，跳过提取: {:?}", actual_target);
        }

        info!("资源 '{}' 处理完成", item.name);
        Ok(())
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
