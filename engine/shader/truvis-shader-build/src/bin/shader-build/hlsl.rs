//! HLSL 着色器编译器
//!
//! 使用 dxc (来自 Vulkan SDK) 将 HLSL 着色器编译为 SPIR-V
//!
//! # 参考
//! - [Vulkan HLSL Guide](https://docs.vulkan.org/guide/latest/hlsl.html)
//! - [Nsight Shader Configuration](https://docs.nvidia.com/nsight-graphics/UserGuide/index.html#configuring-your-application-shaders)

use crate::common::{ShaderCompileTask, ShaderCompiler, ShaderCompilerType, ShaderStage};

/// HLSL 编译器
///
/// 使用 dxc 编译 `.hlsl` 文件
#[derive(Debug, Default)]
pub struct HlslCompiler;

impl HlslCompiler {
    pub const fn new() -> Self {
        Self
    }

    /// 将 ShaderStage 转换为 DXC 的 shader model target
    fn get_shader_model_target(stage: ShaderStage) -> &'static str {
        match stage {
            ShaderStage::Vertex => "vs",
            ShaderStage::TessellationControl => "hs",    // Hull Shader
            ShaderStage::TessellationEvaluation => "ds", // Domain Shader
            ShaderStage::Geometry => "gs",
            ShaderStage::Fragment => "ps", // Pixel Shader
            ShaderStage::Compute => "cs",
            // Ray Tracing 阶段使用 lib
            ShaderStage::RayGen
            | ShaderStage::AnyHit
            | ShaderStage::ClosestHit
            | ShaderStage::Miss
            | ShaderStage::Intersection
            | ShaderStage::RayCallable => "lib",
            ShaderStage::Task => "as", // Amplification Shader
            ShaderStage::Mesh => "ms",
            ShaderStage::General => panic!("DXC does not support Slang shaders"),
        }
    }
}

impl ShaderCompiler for HlslCompiler {
    fn compiler_type(&self) -> ShaderCompilerType {
        ShaderCompilerType::Hlsl
    }

    fn compile(&self, task: &ShaderCompileTask) {
        // Shader Model 版本:
        // - 6.3: 支持 Ray Tracing
        // - 6.5: 支持 Task/Mesh Shader
        // - 6.7: 最新特性
        const SHADER_MODEL: &str = "6_7";
        const ENTRY_POINT: &str = "main";

        let target = Self::get_shader_model_target(task.shader_stage);

        let output = std::process::Command::new("dxc")
            .arg("-spirv")
            .args(["-T", &format!("{target}_{SHADER_MODEL}")])
            // .arg("-Zpc") // 列主序 (column-major)
            .args(["-E", ENTRY_POINT])
            .arg(task.shader_path.as_os_str())
            .arg("-Fo")
            .arg(task.output_path.as_os_str())
            // SPIR-V NonSemantic Shader DebugInfo Instructions，用于 Nsight 调试
            .arg("-fspv-debug=vulkan-with-source")
            .arg("-Zi") // 包含调试信息
            .output()
            .expect("Failed to execute dxc");

        self.process_cmd_output(output);
    }
}
