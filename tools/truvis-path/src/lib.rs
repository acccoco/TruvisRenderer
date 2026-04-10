use std::{
    fs,
    path::{Path, PathBuf},
    sync::OnceLock,
};

use serde::Deserialize;

/// map.toml 中 [dirs] 表的映射
#[derive(Debug, Deserialize)]
struct Dirs {
    engine: String,
    assets: String,
    resources: String,
    tools: String,
    target: String,
    temp: String,
    shader: String,
    cxx: String,
}

#[derive(Debug, Deserialize)]
struct MapConfig {
    dirs: Dirs,
}

// 编译期嵌入 workspace 根目录（由 build.rs 注入）
const WORKSPACE_ROOT: &str = env!("TRUVIS_WORKSPACE_ROOT");

static CONFIG: OnceLock<MapConfig> = OnceLock::new();

fn config() -> &'static MapConfig {
    CONFIG.get_or_init(|| {
        let map_path = Path::new(WORKSPACE_ROOT).join("map.toml");
        let content =
            fs::read_to_string(&map_path).unwrap_or_else(|e| panic!("无法读取 map.toml（{map_path:?}）: {e}"));
        toml::from_str(&content).unwrap_or_else(|e| panic!("map.toml 解析失败: {e}"))
    })
}

/// 统一资源路径管理
///
/// 所有路径基于 workspace 根目录，子目录映射来自根目录 `map.toml`。
/// 路径在首次访问时从 `map.toml` 读取并永久缓存，后续调用零 I/O 开销。
///
/// # 使用示例
/// ```ignore
/// let model   = TruvisPath::assets("sponza.fbx");
/// let texture = TruvisPath::resources("uv_checker.png");
/// let spv     = TruvisPath::shader_build_spv("rt/raygen.slang");
/// ```
pub struct TruvisPath;

impl TruvisPath {
    /// workspace 根目录
    pub fn workspace() -> &'static Path {
        Path::new(WORKSPACE_ROOT)
    }

    /// workspace 根目录（兼容旧名称）
    #[inline]
    pub fn workspace_path() -> PathBuf {
        Self::workspace().to_path_buf()
    }

    /// `target/` 目录
    pub fn target() -> PathBuf {
        Self::workspace().join(&config().dirs.target)
    }

    /// `target/` 目录（兼容旧名称）
    #[inline]
    pub fn target_path() -> PathBuf {
        Self::target()
    }

    /// `.temp/` 目录
    pub fn temp_dir() -> PathBuf {
        Self::workspace().join(&config().dirs.temp)
    }
}

// workspace 根目录下的顶层目录
impl TruvisPath {
    /// `engine/` 目录
    pub fn engine() -> PathBuf {
        Self::workspace().join(&config().dirs.engine)
    }

    /// `engine/` 目录（兼容旧名称）
    #[inline]
    pub fn engine_path() -> PathBuf {
        Self::engine()
    }

    /// `assets/<filename>` 路径
    pub fn assets(filename: &str) -> PathBuf {
        Self::workspace().join(&config().dirs.assets).join(filename)
    }

    /// `assets/<filename>` 路径（兼容旧名称）
    #[inline]
    pub fn assets_path(filename: &str) -> PathBuf {
        Self::assets(filename)
    }

    /// `assets/<filename>` 路径（字符串形式）
    pub fn assets_str(filename: &str) -> String {
        Self::assets(filename).to_str().unwrap().to_string()
    }

    /// `assets/<filename>` 路径（字符串形式，兼容旧名称）
    #[inline]
    pub fn assets_path_str(filename: &str) -> String {
        Self::assets_str(filename)
    }

    /// `resources/<filename>` 路径
    pub fn resources(filename: &str) -> PathBuf {
        Self::workspace().join(&config().dirs.resources).join(filename)
    }

    /// `resources/<filename>` 路径（兼容旧名称）
    #[inline]
    pub fn resources_path(filename: &str) -> PathBuf {
        Self::resources(filename)
    }

    /// `resources/<filename>` 路径（字符串形式）
    pub fn resources_str(filename: &str) -> String {
        Self::resources(filename).to_str().unwrap().to_string()
    }

    /// `resources/<filename>` 路径（字符串形式，兼容旧名称）
    #[inline]
    pub fn resources_path_str(filename: &str) -> String {
        Self::resources_str(filename)
    }

    /// `tools/` 目录
    pub fn tools() -> PathBuf {
        Self::workspace().join(&config().dirs.tools)
    }

    /// `tools/` 目录（兼容旧名称）
    #[inline]
    pub fn tools_path() -> PathBuf {
        Self::tools()
    }
}

// engine 目录下的子目录
impl TruvisPath {
    /// shader 根目录（`engine/shader/`）
    pub fn shader_root() -> PathBuf {
        Self::workspace().join(&config().dirs.shader)
    }

    /// shader 根目录（兼容旧名称）
    #[inline]
    pub fn shader_root_path() -> PathBuf {
        Self::shader_root()
    }

    /// 编译后的 SPIR-V 路径：`engine/shader/.build/<filename>.spv`
    pub fn shader_build_spv(filename: &str) -> String {
        let path = Self::shader_root().join(".build").join(filename);
        let mut s = path.to_str().unwrap().to_string();
        s.push_str(".spv");
        s
    }

    /// 编译后的 SPIR-V 路径（兼容旧名称）
    #[inline]
    pub fn shader_build_path_str(filename: &str) -> String {
        Self::shader_build_spv(filename)
    }

    /// cxx 根目录（`engine/cxx/`）
    pub fn cxx_root() -> PathBuf {
        Self::workspace().join(&config().dirs.cxx)
    }

    /// cxx 根目录（兼容旧名称）
    #[inline]
    pub fn cxx_root_path() -> PathBuf {
        Self::cxx_root()
    }
}
