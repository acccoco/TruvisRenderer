use truvis_cxx_binding_codegen::{CxxBindingGenerator, CxxBindingSpec};
use truvis_path::TruvisPath;

fn main() {
    let module_dir = TruvisPath::workspace().join("cxx/modules/truvixx-streamline");
    let generated_include_dir = TruvisPath::target().join("cxx/generated/include");
    let generated = CxxBindingGenerator::new(CxxBindingSpec {
        output_crate: "truvis-streamline-binding",
        header: module_dir.join("include/TruvixxStreamline/c_api/module.h"),
        include_roots: vec![module_dir.join("include"), generated_include_dir],
        allowlist_types: &["Truvixx.*"],
        allowlist_functions: &["truvixx_.*"],
        allowlist_vars: &["Truvixx.*"],
        module_raw_lines: &[],
        layout_tests: true,
    })
    .generate();

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rustc-env=TRUVIS_STREAMLINE_BINDINGS_RS={}", generated.source_path.display());
    println!("cargo:rustc-env=TRUVIS_STREAMLINE_BINDINGS_HASH={}", generated.content_hash);
    println!("cargo:rustc-env=TRUVIS_STREAMLINE_BINDINGS_HASH_FILE={}", generated.hash_path.display());

    let profile = std::env::var("PROFILE").expect("PROFILE must be set by Cargo");
    println!("cargo:rustc-link-search=native={}", TruvisPath::target().join(profile).display());
    println!("cargo:rustc-link-lib=dylib=truvixx-streamline-capi");
}
