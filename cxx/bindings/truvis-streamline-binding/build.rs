use truvis_cxx_binding_build::{CxxBindingGenerator, CxxBindingSpec, NativeLibrary};
use truvis_path::TruvisPath;

fn main() {
    let native = NativeLibrary::new();
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

    native.link("truvixx-streamline-capi");
    native.copy_runtime(native.output_dir(), &["truvixx-streamline-capi.dll"]);

    let sdk_bin =
        TruvisPath::external().join("streamline-sdk/bin/x64").join(if native.is_debug() { "development" } else { "" });
    native.copy_runtime(
        &sdk_bin,
        &[
            "sl.interposer.dll",
            "sl.common.dll",
            "sl.pcl.dll",
            "sl.dlss.dll",
            "nvngx_dlss.dll",
            "sl.dlss_d.dll",
            "nvngx_dlssd.dll",
            "NvLowLatencyVk.dll",
        ],
    );
    if native.is_debug() {
        // 跟踪目录以发现可选 DLL，避免追踪不存在的文件而每次重跑。
        println!("cargo:rerun-if-changed={}", sdk_bin.display());
        let imgui = sdk_bin.join("sl.imgui.dll");
        if imgui.is_file() {
            native.copy_file(&imgui);
        }
    }
    native.copy_runtime(
        &module_dir.join("runtime"),
        &[
            "sl.common.json",
            "sl.imgui.json",
            "sl.interposer.json",
            "sl.reflex.json",
        ],
    );
}
