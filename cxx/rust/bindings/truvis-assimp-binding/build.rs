use truvis_cxx_binding_codegen::{CxxBindingGenerator, CxxBindingSpec};
use truvis_path::TruvisPath;

fn main() {
    let module_dir = TruvisPath::workspace().join("cxx/modules/truvixx-assimp");
    let generated_include_dir = TruvisPath::target().join("cxx/generated/include");
    let generated = CxxBindingGenerator::new(CxxBindingSpec {
        output_crate: "truvis-assimp-binding",
        header: module_dir.join("include/TruvixxAssimp/c_api/module.h"),
        include_roots: vec![module_dir.join("include"), generated_include_dir],
        allowlist_types: &["Truvixx.*", "ResType"],
        allowlist_functions: &["truvixx_.*"],
        allowlist_vars: &["ResType_.*"],
    })
    .generate();

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rustc-env=TRUVIS_ASSIMP_BINDINGS_RS={}", generated.source_path.display());
    println!("cargo:rustc-env=TRUVIS_ASSIMP_BINDINGS_HASH={}", generated.content_hash);
    println!("cargo:rustc-env=TRUVIS_ASSIMP_BINDINGS_HASH_FILE={}", generated.hash_path.display());

    let profile = std::env::var("PROFILE").expect("PROFILE must be set by Cargo");
    println!("cargo:rustc-link-search=native={}", TruvisPath::target().join(profile).display());
    println!("cargo:rustc-link-lib=dylib=truvixx-assimp-capi");
}
