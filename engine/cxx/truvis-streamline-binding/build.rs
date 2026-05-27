use truvis_path::TruvisPath;

/// 读取 Streamline C API 头文件，输出到当前 crate 中。
fn gen_rust_binding() {
    let cxx_root_path = TruvisPath::cxx_root_path();
    let module_path = cxx_root_path.join("mods/truvixx-streamline");

    let bindings = bindgen::Builder::default()
        .header(module_path.join("include/TruvixxStreamline/c_api/module.h").to_str().unwrap())
        .clang_args([format!("-I{}", module_path.join("include").to_str().unwrap())])
        // 任何被包含的头文件变化时，都通知 cargo 重新构建当前 crate。
        .raw_line("#![allow(clippy::all)]")
        .raw_line("#![allow(warnings)]")
        .derive_default(true)
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .enable_cxx_namespaces()
        .generate()
        .expect("Unable to generate bindings");

    let out_path = std::path::PathBuf::from("src").join("_ffi_bindings.rs");
    bindings.write_to_file(out_path).expect("Couldn't write bindings!");
}

fn main() {
    let cxx_root_path = TruvisPath::cxx_root_path();
    let module_path = cxx_root_path.join("mods/truvixx-streamline");

    println!("cargo:rerun-if-changed={}", cxx_root_path.join("CMakeLists.txt").display());
    println!("cargo:rerun-if-changed={}", cxx_root_path.join("vcpkg.json").display());
    println!("cargo:rerun-if-changed={}", module_path.display());
    println!("cargo:rerun-if-changed=build.rs");

    gen_rust_binding();

    let build_type = std::env::var("PROFILE").unwrap();
    let cargo_build_dir = TruvisPath::target_path().join(build_type);

    println!("cargo:rustc-link-search=native={}", cargo_build_dir.display());
    println!("cargo:rustc-link-lib=dylib=truvixx-streamline-capi");
}
