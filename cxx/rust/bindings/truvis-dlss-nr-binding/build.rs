use truvis_cxx_binding_codegen::{CxxBindingGenerator, CxxBindingSpec};
use truvis_path::TruvisPath;

fn main() {
    assert_eq!(std::env::var("TARGET").unwrap(), "x86_64-pc-windows-msvc");
    let module = TruvisPath::workspace().join("cxx/modules/truvixx-dlss-nr");
    let generated = CxxBindingGenerator::new(CxxBindingSpec {
        output_crate: "truvis-dlss-nr-binding",
        header: module.join("include/TruvixxDlssNr/c_api/module.h"),
        include_roots: vec![
            module.join("include"),
            TruvisPath::target().join("cxx/generated/include"),
        ],
        allowlist_types: &["TruvixxNr.*"],
        allowlist_functions: &["truvixx_nr_.*"],
        allowlist_vars: &["TruvixxNr.*"],
        module_raw_lines: &[],
        layout_tests: false,
    })
    .generate();
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rustc-env=TRUVIS_NR_BINDINGS_RS={}", generated.source_path.display());
    println!("cargo:rustc-env=TRUVIS_NR_BINDINGS_HASH={}", generated.content_hash);
    println!("cargo:rustc-env=TRUVIS_NR_BINDINGS_HASH_FILE={}", generated.hash_path.display());
    let profile = std::env::var("PROFILE").unwrap();
    println!("cargo:rustc-link-search=native={}", TruvisPath::target().join(profile).display());
    println!("cargo:rustc-link-lib=dylib=truvixx-dlss-nr-capi");
}
