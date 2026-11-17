use truvis_cxx_binding_build::{CxxBindingGenerator, CxxBindingSpec, NativeLibrary};
use truvis_path::TruvisPath;

fn main() {
    let native = NativeLibrary::new();
    let module_dir = TruvisPath::workspace().join("cxx/modules/truvixx-assimp");
    let generated_include_dir = TruvisPath::target().join("cxx/generated/include");
    let generated = CxxBindingGenerator::new(CxxBindingSpec {
        output_crate: "truvis-assimp-binding",
        header: module_dir.join("include/TruvixxAssimp/c_api/module.h"),
        include_roots: vec![module_dir.join("include"), generated_include_dir],
        allowlist_types: &["Truvixx.*", "ResType"],
        allowlist_functions: &["truvixx_.*"],
        allowlist_vars: &["ResType_.*"],
        module_raw_lines: &[],
        layout_tests: true,
    })
    .generate();

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rustc-env=TRUVIS_ASSIMP_BINDINGS_RS={}", generated.source_path.display());
    println!("cargo:rustc-env=TRUVIS_ASSIMP_BINDINGS_HASH={}", generated.content_hash);
    println!("cargo:rustc-env=TRUVIS_ASSIMP_BINDINGS_HASH_FILE={}", generated.hash_path.display());

    native.link("truvixx-assimp-capi");
    native.copy_runtime(native.output_dir(), &["truvixx-assimp-capi.dll"]);

    // 仅 Assimp 的工具集后缀采用唯一匹配；间接 DLL 显式维护，升级 SDK 时人工核对。
    let directory = native.vcpkg_bin_dir();
    println!("cargo:rerun-if-changed={}", directory.display());
    let suffix = if native.is_debug() { "-mtd.dll" } else { "-mt.dll" };
    let assimp_dlls: Vec<_> = std::fs::read_dir(directory)
        .expect("Cannot read selected vcpkg runtime directory")
        .map(|entry| entry.expect("Cannot read vcpkg directory entry").path())
        .filter(|path| {
            path.file_name().is_some_and(|name| {
                let name = name.to_string_lossy();
                name.starts_with("assimp-") && name.ends_with(suffix)
            })
        })
        .collect();
    assert_eq!(assimp_dlls.len(), 1, "Expected one assimp-*{suffix} in {}", directory.display());
    native.copy_file(&assimp_dlls[0]);
    native.copy_runtime(
        directory,
        &[
            "kubazip.dll",
            "minizip.dll",
            "poly2tri.dll",
            "pugixml.dll",
            if native.is_debug() { "zlibd1.dll" } else { "zlib1.dll" },
        ],
    );
}
