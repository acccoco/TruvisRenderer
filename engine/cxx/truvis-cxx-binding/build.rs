use truvis_path::TruvisPath;

/// 读取 c++ 头文件（只能有一个），输出到当前 crate 中
fn gen_rust_binding() {
    let cxx_root_path = TruvisPath::cxx_root_path();

    let bindings = bindgen::Builder::default()
        .header(cxx_root_path.join("mods/truvixx-interface/include/TruvixxInterface/lib.h").to_str().unwrap())
        .clang_args([
            format!("-I{}", cxx_root_path.join("mods/truvixx-interface/include").to_str().unwrap()),
            format!("-I{}", cxx_root_path.join("mods/truvixx-assimp/include").to_str().unwrap()),
        ])
        // Tell cargo to invalidate the built crate whenever any of the
        // included header files changed.
        .raw_line("#![allow(clippy::all)]")
        .raw_line("#![allow(warnings)]")
        .derive_default(true)
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .enable_cxx_namespaces()
        .generate()
        .expect("Unable to generate bindings");

    // Write the bindings to the $OUT_DIR/bindings.rs file.
    let out_path = std::path::PathBuf::from("src").join("_ffi_bindings.rs");
    bindings.write_to_file(out_path).expect("Couldn't write bindings!");
}

/// 强制执行的方法: touch build.rs; cargo build
fn main() {
    println!("cargo:rerun-if-changed=cxx/CMakeLists.txt");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=cxx/vcpkg.json");
    println!("cargo:rerun-if-changed=cxx/truvixx-assimp");
    println!("cargo:rerun-if-changed=cxx/truvixx-interface");

    // 将自动绑定文件写入到当前项目中
    gen_rust_binding();

    let build_type = std::env::var("PROFILE").unwrap();

    let cargo_build_dir = TruvisPath::target_path().join(build_type);
    // println!("cargo:warning=link-search-dir: {}", cargo_build_dir.display());

    println!("cargo:rustc-link-search=native={}", cargo_build_dir.display());
    let libs = ["truvixx-interface"];
    for lib in libs {
        println!("cargo:rustc-link-lib=static={}", lib);
    }
}
