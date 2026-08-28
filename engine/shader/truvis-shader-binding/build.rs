use truvis_path::TruvisPath;
use truvis_shader_binding_codegen::{BindingGenerator, BindingSpec};
use truvis_shader_manifest::ShaderManifest;

fn main() {
    let manifest =
        ShaderManifest::load(TruvisPath::shader_manifest_path()).expect("Unable to load shader package manifest");
    let target = std::env::var("TARGET").expect("TARGET must be set by Cargo build scripts");
    let binding = manifest.resolved_binding("engine", &target).expect("Unable to resolve engine shader binding");
    let generated = BindingGenerator::new(BindingSpec {
        binding,
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
    })
    .generate();

    println!("cargo:rustc-env=TRUVIS_SHADER_BINDINGS_RS={}", generated.source_path.display());
    println!("cargo:rustc-env=TRUVIS_SHADER_BINDINGS_HASH={}", generated.content_hash);
    println!("cargo:rustc-env=TRUVIS_SHADER_BINDINGS_HASH_FILE={}", generated.hash_path.display());
}
