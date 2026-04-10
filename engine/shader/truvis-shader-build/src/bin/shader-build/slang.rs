//! Slang 着色器编译器
//!
//! 使用 slangc 将 Slang 着色器编译为 SPIR-V

use crate::common::{EnvPath, ShaderCompileTask, ShaderCompiler, ShaderCompilerType};

/// Slang 编译器
///
/// 使用 slangc 编译 `.slang` 文件
#[derive(Debug, Default)]
pub struct SlangCompiler;

impl SlangCompiler {
    pub const fn new() -> Self {
        Self
    }
}

impl ShaderCompiler for SlangCompiler {
    fn compiler_type(&self) -> ShaderCompilerType {
        ShaderCompilerType::Slang
    }

    fn compile(&self, task: &ShaderCompileTask) {
        let output = std::process::Command::new(EnvPath::slangc_path())
            .args([
                "-I",
                EnvPath::shader_root_path().to_str().unwrap(),
                "-g2",                         // 生成 debug info (默认是 g2)
                "-matrix-layout-column-major", // 列主序
                "-fvk-use-entrypoint-name",    // 具有多个 entry 时需要此选项
                "-target",
                "spirv", // 如果想要输出字节码：spirv-asm
                "-o",
                task.output_path.to_str().unwrap(),
                task.shader_path.to_str().unwrap(),
            ])
            .output()
            .expect("Failed to execute slangc");

        self.process_cmd_output(output);
    }
}
