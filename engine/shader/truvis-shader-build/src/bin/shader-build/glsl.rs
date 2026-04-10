//! GLSL 着色器编译器
//!
//! 使用 glslc (来自 Vulkan SDK) 将 GLSL 着色器编译为 SPIR-V

use crate::common::{EnvPath, ShaderCompileTask, ShaderCompiler, ShaderCompilerType};

/// GLSL 编译器
///
/// 使用 glslc 编译 `.vert`, `.frag`, `.comp`, `.rgen`, `.rchit`, `.rmiss` 等 GLSL 文件
#[derive(Debug, Default)]
pub struct GlslCompiler;

impl GlslCompiler {
    pub const fn new() -> Self {
        Self
    }
}

impl ShaderCompiler for GlslCompiler {
    fn compiler_type(&self) -> ShaderCompilerType {
        ShaderCompilerType::Glsl
    }

    fn compile(&self, task: &ShaderCompileTask) {
        let output = std::process::Command::new("glslc")
            .args([
                &format!("-I{:?}", EnvPath::shader_root_path()),
                "-g", // 生成调试信息
                "--target-env=vulkan1.2",
                "--target-spv=spv1.4", // Ray tracing 最低版本为 spv1.4
                "-o",
                task.output_path.to_str().unwrap(),
                task.shader_path.to_str().unwrap(),
            ])
            .output()
            .expect("Failed to execute glslc");

        self.process_cmd_output(output);
    }
}
