//! 着色器编译的共享类型和工具

use std::sync::OnceLock;
use truvis_crate_tools::resource::TruvisPath;

/// Shader 的执行阶段
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShaderStage {
    Vertex,
    /// HLSL Hull shader
    TessellationControl,
    /// HLSL Domain shader
    TessellationEvaluation,
    Geometry,
    /// HLSL Pixel shader
    Fragment,
    Compute,

    // Ray Tracing 阶段
    RayGen,
    AnyHit,
    ClosestHit,
    Miss,
    Intersection,
    RayCallable,

    /// HLSL Amplification shader
    Task,
    Mesh,

    /// Slang 不需要明确的 shader stage
    General,
}

/// 着色器编译器类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShaderCompilerType {
    Glsl,
    Hlsl,
    Slang,
}

/// 当前项目的环境路径，基于 workspace 根目录
pub struct EnvPath;

impl EnvPath {
    pub fn shader_root_path() -> &'static std::path::Path {
        static PATH: OnceLock<std::path::PathBuf> = OnceLock::new();
        PATH.get_or_init(TruvisPath::shader_root_path)
    }

    pub fn shader_entry_path() -> &'static std::path::Path {
        static PATH: OnceLock<std::path::PathBuf> = OnceLock::new();
        PATH.get_or_init(|| TruvisPath::shader_root_path().join("entry"))
    }

    /// 编译 shader 的输出路径
    pub fn shader_build_path() -> &'static std::path::Path {
        static PATH: OnceLock<std::path::PathBuf> = OnceLock::new();
        PATH.get_or_init(|| TruvisPath::shader_root_path().join(".build"))
    }

    pub fn shader_share_path() -> &'static std::path::Path {
        static PATH: OnceLock<std::path::PathBuf> = OnceLock::new();
        PATH.get_or_init(|| TruvisPath::shader_root_path().join("share"))
    }

    /// Slang 编译器路径
    pub fn slangc_path() -> &'static std::path::Path {
        static PATH: OnceLock<std::path::PathBuf> = OnceLock::new();
        PATH.get_or_init(|| {
            let mut path = TruvisPath::tools_path();
            path.extend(["slang", "bin", "slangc.exe"]);
            path
        })
    }
}

/// 着色器编译器 Trait
pub trait ShaderCompiler: Send + Sync {
    /// 返回编译器类型
    #[allow(dead_code)]
    fn compiler_type(&self) -> ShaderCompilerType;

    /// 编译着色器
    fn compile(&self, task: &ShaderCompileTask);

    /// 根据 cmd 执行的结果，处理输出信息
    fn process_cmd_output(&self, output: std::process::Output) {
        if !output.stdout.is_empty() {
            log::info!("stdout: {}", String::from_utf8_lossy(&output.stdout));
        }
        if !output.stderr.is_empty() {
            log::error!("stderr: {}", String::from_utf8_lossy(&output.stderr));
        }
    }
}

/// 一个具体的编译任务
#[derive(Debug)]
pub struct ShaderCompileTask {
    pub shader_path: std::path::PathBuf,
    pub output_path: std::path::PathBuf,
    pub shader_stage: ShaderStage,
    pub compiler_type: ShaderCompilerType,
}

impl ShaderCompileTask {
    /// 从目录项创建编译任务
    ///
    /// # Arguments
    /// * `entry` - 相对于 workspace 的目录项
    ///
    /// # Returns
    /// 如果文件扩展名不被支持，返回 None
    pub fn new(entry: &walkdir::DirEntry) -> Option<Self> {
        let shader_path = entry.path().to_str()?.replace('\\', "/");
        let shader_path = std::path::Path::new(&shader_path);

        // 相对于 shader src 的路径
        let relative_path = shader_path.strip_prefix(EnvPath::shader_entry_path()).ok()?;
        let shader_name = entry.file_name().to_str()?;

        // 构造输出路径
        let mut output_path = EnvPath::shader_build_path().join(relative_path);
        let mut new_ext = output_path.extension()?.to_os_string();
        new_ext.push(".spv");
        output_path.set_extension(new_ext);

        let shader_stage = Self::parse_shader_stage(shader_name)?;
        let compiler_type = Self::select_compiler(shader_name);

        Some(Self {
            shader_path: shader_path.to_path_buf(),
            output_path,
            shader_stage,
            compiler_type,
        })
    }

    /// 根据文件名解析 shader stage
    fn parse_shader_stage(shader_name: &str) -> Option<ShaderStage> {
        let stage = match () {
            // Vertex shaders
            _ if shader_name.ends_with(".vert") || shader_name.ends_with(".vs.hlsl") => ShaderStage::Vertex,
            // Fragment/Pixel shaders
            _ if shader_name.ends_with(".frag") || shader_name.ends_with(".ps.hlsl") => ShaderStage::Fragment,
            // Compute shaders
            _ if shader_name.ends_with(".comp") || shader_name.ends_with(".cs.hlsl") => ShaderStage::Compute,
            // Ray Tracing shaders
            _ if shader_name.ends_with(".rgen") => ShaderStage::RayGen,
            _ if shader_name.ends_with(".rchit") => ShaderStage::ClosestHit,
            _ if shader_name.ends_with(".rmiss") => ShaderStage::Miss,
            _ if shader_name.ends_with(".rahit") => ShaderStage::AnyHit,
            _ if shader_name.ends_with(".rint") => ShaderStage::Intersection,
            _ if shader_name.ends_with(".rcall") => ShaderStage::RayCallable,
            // Tessellation shaders
            _ if shader_name.ends_with(".tesc") || shader_name.ends_with(".hs.hlsl") => {
                ShaderStage::TessellationControl
            }
            _ if shader_name.ends_with(".tese") || shader_name.ends_with(".ds.hlsl") => {
                ShaderStage::TessellationEvaluation
            }
            // Geometry shader
            _ if shader_name.ends_with(".geom") || shader_name.ends_with(".gs.hlsl") => ShaderStage::Geometry,
            // Mesh shaders
            _ if shader_name.ends_with(".task") || shader_name.ends_with(".as.hlsl") => ShaderStage::Task,
            _ if shader_name.ends_with(".mesh") || shader_name.ends_with(".ms.hlsl") => ShaderStage::Mesh,
            // Slang (通用)
            _ if shader_name.ends_with(".slang") => ShaderStage::General,
            _ => return None,
        };

        Some(stage)
    }

    /// 根据文件扩展名选择编译器
    fn select_compiler(shader_name: &str) -> ShaderCompilerType {
        if shader_name.ends_with(".hlsl") {
            ShaderCompilerType::Hlsl
        } else if shader_name.ends_with(".slang") {
            ShaderCompilerType::Slang
        } else {
            ShaderCompilerType::Glsl
        }
    }
}
