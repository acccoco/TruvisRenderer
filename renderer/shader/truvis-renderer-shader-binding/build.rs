use std::path::PathBuf;

use truvis_path::TruvisPath;
use truvis_shader_binding_codegen::{BindingGenerator, BindingSpec, ModuleRawLine};

fn main() {
    let workspace_root = TruvisPath::workspace_path();
    let engine_shader_root = TruvisPath::shader_root_path();
    let renderer_shader_root = workspace_root.join("renderer").join("shader");
    let engine_ffi = engine_shader_root.join("truvis-shader-binding").join("ffi");
    let header = PathBuf::from("./ffi/rust_ffi.hpp");
    let include_dirs = [engine_shader_root, renderer_shader_root.clone(), engine_ffi];
    let module_raw_lines = [ModuleRawLine {
        module: "root",
        line: "pub use truvis_shader_binding::gpu::{Float2, Float3, Float4, Float4x4, Int2, Int3, Int4, Uint, Uint2, Uint3, Uint4};",
    }];
    let rerun_inputs = [renderer_shader_root.join("abi").join("renderer")];

    let generated = BindingGenerator::new(BindingSpec {
        header: &header,
        include_dirs: &include_dirs,
        output_crate: "truvis-renderer-shader-binding",
        allowlist_types: &["renderer::.*"],
        allowlist_vars: &["renderer::.*"],
        module_raw_lines: &module_raw_lines,
        rerun_if_changed: &rerun_inputs,
    })
    .generate();

    println!("cargo:rustc-env=TRUVIS_RENDERER_SHADER_BINDINGS_RS={}", generated.source_path.display());
    println!("cargo:rustc-env=TRUVIS_RENDERER_SHADER_BINDINGS_HASH={}", generated.content_hash);
    println!("cargo:rustc-env=TRUVIS_RENDERER_SHADER_BINDINGS_HASH_FILE={}", generated.hash_path.display());
}
