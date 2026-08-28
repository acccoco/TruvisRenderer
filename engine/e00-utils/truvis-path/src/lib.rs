use std::{
    fs,
    path::{Path, PathBuf},
    sync::OnceLock,
};

use serde::Deserialize;

mod path_utils;
pub use path_utils::PathUtils;

/// paths.toml 中 [dirs] 表的映射
#[derive(Debug, Deserialize)]
struct Dirs {
    engine: String,
    assets: String,
    resources: String,
    config: String,
    external: String,
    target: String,
    temp: String,
    shader_build: String,
    binding_build: String,
    cxx: String,
}

#[derive(Debug, Deserialize)]
struct PathsConfig {
    dirs: Dirs,
}

// 编译期嵌入 workspace 根目录（由 build.rs 注入）
const WORKSPACE_ROOT: &str = env!("TRUVIS_WORKSPACE_ROOT");

static PATHS_CONFIG: OnceLock<PathsConfig> = OnceLock::new();

fn paths_config() -> &'static PathsConfig {
    PATHS_CONFIG.get_or_init(|| {
        let paths_path = Path::new(WORKSPACE_ROOT).join("paths.toml");
        let content =
            fs::read_to_string(&paths_path).unwrap_or_else(|e| panic!("无法读取 paths.toml（{paths_path:?}）: {e}"));
        toml::from_str(&content).unwrap_or_else(|e| panic!("paths.toml 解析失败: {e}"))
    })
}

/// 统一资源路径管理
///
/// 所有路径基于 workspace 根目录，子目录映射来自根目录 `paths.toml`。
/// 路径在首次访问时从 `paths.toml` 读取并永久缓存，后续调用零 I/O 开销。
///
/// # 使用示例
/// ```ignore
/// let model   = TruvisPath::assets("sponza.fbx");
/// let texture = TruvisPath::resources("sky.jpg"); // assets/resources/sky.jpg
/// let shader_root = TruvisPath::shader_build_dir(); // build/shader/
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

    /// Cargo 输出目录。
    ///
    /// 函数名保留 `target` 是为了兼容旧调用点；实际目录来自 `paths.toml`，
    /// 当前配置为 workspace 下的 `build/`。
    pub fn target() -> PathBuf {
        Self::workspace().join(&paths_config().dirs.target)
    }

    /// Cargo 输出目录（兼容旧名称）。
    #[inline]
    pub fn target_path() -> PathBuf {
        Self::target()
    }

    /// `.temp/` 目录
    pub fn temp_dir() -> PathBuf {
        Self::workspace().join(&paths_config().dirs.temp)
    }
}

// workspace 根目录下的顶层目录
impl TruvisPath {
    /// `engine/` 目录
    pub fn engine() -> PathBuf {
        Self::workspace().join(&paths_config().dirs.engine)
    }

    /// `engine/` 目录（兼容旧名称）
    #[inline]
    pub fn engine_path() -> PathBuf {
        Self::engine()
    }

    /// `assets/<filename>` 路径
    pub fn assets(filename: &str) -> PathBuf {
        Self::workspace().join(&paths_config().dirs.assets).join(filename)
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

    /// `assets/resources/<filename>` 路径
    pub fn resources(filename: &str) -> PathBuf {
        Self::workspace().join(&paths_config().dirs.resources).join(filename)
    }

    /// `assets/resources/<filename>` 路径（兼容旧名称）
    #[inline]
    pub fn resources_path(filename: &str) -> PathBuf {
        Self::resources(filename)
    }

    /// `assets/resources/<filename>` 路径（字符串形式）
    pub fn resources_str(filename: &str) -> String {
        Self::resources(filename).to_str().unwrap().to_string()
    }

    /// `assets/resources/<filename>` 路径（字符串形式，兼容旧名称）
    #[inline]
    pub fn resources_path_str(filename: &str) -> String {
        Self::resources_str(filename)
    }

    /// 项目维护的外部工具与 runtime 配置目录（`config/`）。
    pub fn config() -> PathBuf {
        Self::workspace().join(&paths_config().dirs.config)
    }

    /// 下载或检出的第三方工具、SDK 与参考源码目录（`external/`）。
    pub fn external() -> PathBuf {
        Self::workspace().join(&paths_config().dirs.external)
    }
}

// engine 目录下的子目录
impl TruvisPath {
    /// 编译后的 shader 产物目录（`build/shader/`）。
    pub fn shader_build_dir() -> PathBuf {
        Self::workspace().join(&paths_config().dirs.shader_build)
    }

    /// shader package manifest 路径。
    pub fn shader_manifest_path() -> PathBuf {
        Self::workspace().join("shader-packages.toml")
    }

    /// Rust 自动绑定生成目录（`build/bindings/`）。
    ///
    /// 该目录保存 bindgen 生成的 Rust 源文件，和 `build/shader/`、`build/cxx/`
    /// 一样属于 workspace 级构建产物；具体 crate 需要继续按 target 和模块名拆分子目录，
    /// 避免不同 FFI / shader binding 互相覆盖。
    pub fn rust_binding_build_dir() -> PathBuf {
        Self::workspace().join(&paths_config().dirs.binding_build)
    }

    /// cxx 根目录（`engine/cxx/`）
    pub fn cxx_root() -> PathBuf {
        Self::workspace().join(&paths_config().dirs.cxx)
    }

    /// cxx 根目录（兼容旧名称）
    #[inline]
    pub fn cxx_root_path() -> PathBuf {
        Self::cxx_root()
    }
}
