use std::path::PathBuf;

use truvis_path::TruvisPath;
use truvis_shader_binding_codegen::{BindingGenerator, BindingSpec};

fn main() {
    let shader_root = TruvisPath::shader_root_path();
    let header = PathBuf::from("./ffi/rust_ffi.hpp");
    let generated = BindingGenerator::new(BindingSpec {
        header: &header,
        include_dirs: std::slice::from_ref(&shader_root),
        output_crate: "truvis-shader-binding",
        allowlist_types: &[
            "uint",
            "float2",
            "float3",
            "float4",
            "float4x4",
            "int2",
            "int3",
            "int4",
            "uint2",
            "uint3",
            "uint4",
            "engine::.*",
        ],
        allowlist_vars: &["engine::.*"],
        module_raw_lines: &[],
        rerun_if_changed: &[shader_root.join("abi").join("engine")],
    })
    .generate();

    println!("cargo:rustc-env=TRUVIS_SHADER_BINDINGS_RS={}", generated.source_path.display());
    println!("cargo:rustc-env=TRUVIS_SHADER_BINDINGS_HASH={}", generated.content_hash);
    println!("cargo:rustc-env=TRUVIS_SHADER_BINDINGS_HASH_FILE={}", generated.hash_path.display());
}
