//! Shader 编译工具
//!
//! 将指定目录下的所有 shader 文件编译为 SPIR-V 文件，输出到 `.build` 目录

mod common;
mod glsl;
mod hlsl;
mod slang;

use common::{EnvPath, ShaderCompileTask, ShaderCompiler, ShaderCompilerType};
use glsl::GlslCompiler;
use hlsl::HlslCompiler;
use rayon::prelude::*;
use slang::SlangCompiler;
use truvis_crate_tools::init_log::init_log;

/// 根据编译器类型获取对应的编译器实例
fn get_compiler(compiler_type: ShaderCompilerType) -> Box<dyn ShaderCompiler> {
    match compiler_type {
        ShaderCompilerType::Glsl => Box::new(GlslCompiler::new()),
        ShaderCompilerType::Hlsl => Box::new(HlslCompiler::new()),
        ShaderCompilerType::Slang => Box::new(SlangCompiler::new()),
    }
}

fn main() {
    init_log();

    log::info!("Shader include path: {:?}", EnvPath::shader_share_path());
    log::info!("Shader entry path: {:?}", EnvPath::shader_entry_path());
    log::info!("Shader output path: {:?}", EnvPath::shader_build_path());

    // 编译 shader 目录下的所有 shader 文件
    walkdir::WalkDir::new(EnvPath::shader_entry_path())
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_file())
        .filter_map(|entry| ShaderCompileTask::new(&entry))
        .par_bridge() // 并行化编译
        .for_each(|task| {
            log::info!("Compiling shader: {:?}", task.shader_path);

            // 确保输出目录存在
            if let Some(parent) = task.output_path.parent() {
                std::fs::create_dir_all(parent).ok();
            }

            let compiler = get_compiler(task.compiler_type);
            compiler.compile(&task);
        });

    log::info!("Shader compilation completed.");
}
