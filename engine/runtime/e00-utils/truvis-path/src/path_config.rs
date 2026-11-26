use std::path::{Path, PathBuf};

const WORKSPACE_ROOT: &str = env!("TRUVIS_WORKSPACE_ROOT");

/// 工作区目录配置。每个字段保存单级目录名，由 `TruvisPath` 按父子关系组合。
struct PathsConfig {
    engine: &'static str,
    assets: &'static str,
    resources: &'static str,
    config: &'static str,
    external: &'static str,
    target: &'static str,
    temp: &'static str,
    shader_build: &'static str,
    binding_build: &'static str,
}

impl PathsConfig {
    const fn get() -> &'static Self {
        &Self {
            engine: "engine",
            assets: "assets",
            resources: "resources",
            config: "config",
            external: "external",
            target: "build",
            temp: ".temp",
            shader_build: "shader",
            binding_build: "bindings",
        }
    }
}

/// 统一资源路径管理。
///
/// 工作区绝对路径由 build script 注入，运行时与构建工具基于同一目录布局。
///
/// # 使用示例
/// ```ignore
/// let model = TruvisPath::assets("sponza.fbx");
/// let texture = TruvisPath::resources("sky.jpg"); // assets/resources/sky.jpg
/// let shader_root = TruvisPath::shader_build_dir(); // build/shader/
/// ```
pub struct TruvisPath;

impl TruvisPath {
    /// 工作区根目录。
    pub fn workspace() -> &'static Path {
        Path::new(WORKSPACE_ROOT)
    }

    /// 与 Cargo 和 CMake 共用的工作区构建目录（`build/`）。
    pub fn target_dir() -> PathBuf {
        Self::workspace().join(PathsConfig::get().target)
    }

    /// 临时文件目录（`.temp/`）。
    pub fn temp_dir() -> PathBuf {
        Self::workspace().join(PathsConfig::get().temp)
    }

    /// Engine 源码目录（`engine/`）。
    pub fn engine() -> PathBuf {
        Self::workspace().join(PathsConfig::get().engine)
    }

    /// `assets/<filename>` 路径。
    pub fn assets(filename: &str) -> PathBuf {
        Self::workspace().join(PathsConfig::get().assets).join(filename)
    }

    /// `assets/resources/<filename>` 路径。
    pub fn resources(filename: &str) -> PathBuf {
        Self::assets(PathsConfig::get().resources).join(filename)
    }

    /// 项目维护的外部工具与 runtime 配置目录（`config/`）。
    pub fn config() -> PathBuf {
        Self::workspace().join(PathsConfig::get().config)
    }

    /// 下载或检出的第三方工具、SDK 与参考源码目录（`external/`）。
    pub fn external() -> PathBuf {
        Self::workspace().join(PathsConfig::get().external)
    }

    /// 编译后的 shader 产物目录（`build/shader/`）。
    pub fn shader_build_dir() -> PathBuf {
        Self::target_dir().join(PathsConfig::get().shader_build)
    }

    /// Shader package manifest 路径。
    pub fn shader_manifest_path() -> PathBuf {
        Self::workspace().join("shader-packages.toml")
    }

    /// Rust 自动绑定生成目录（`build/bindings/`）。
    ///
    /// 具体 crate 按 target 和模块名拆分子目录，避免不同 FFI 和 shader binding 互相覆盖。
    pub fn rust_binding_build_dir() -> PathBuf {
        Self::target_dir().join(PathsConfig::get().binding_build)
    }
}
