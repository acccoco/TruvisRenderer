use truvis_shader_binding_codegen::{BindingGenerator, BindingSpec, ModuleRawLine};
use truvis_shader_manifest::ShaderManifest;

fn main() {
    let manifest = ShaderManifest::load_default().expect("Unable to load shader package manifest");
    let target = std::env::var("TARGET").expect("TARGET must be set by Cargo build scripts");
    let binding = manifest.resolved_binding("renderer", &target).expect("Unable to resolve renderer shader binding");
    let module_raw_lines = [ModuleRawLine {
        module: "root",
        line: "pub use truvis_shader_binding::gpu::{Float2, Float3, Float4, Float4x4, Int2, Int3, Int4, Uint, Uint2, Uint3, Uint4};",
    }];
    let generated = BindingGenerator::new(BindingSpec {
        binding,
        allowlist_types: &["renderer::.*"],
        allowlist_vars: &["renderer::.*"],
        module_raw_lines: &module_raw_lines,
    })
    .generate();

    println!("cargo:rustc-env=TRUVIS_RENDERER_SHADER_BINDINGS_RS={}", generated.source_path.display());
    println!("cargo:rustc-env=TRUVIS_RENDERER_SHADER_BINDINGS_HASH={}", generated.content_hash);
    println!("cargo:rustc-env=TRUVIS_RENDERER_SHADER_BINDINGS_HASH_FILE={}", generated.hash_path.display());
}
