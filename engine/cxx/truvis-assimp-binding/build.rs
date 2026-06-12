use truvis_path::TruvisPath;

fn write_binding_if_changed(bindings: bindgen::Bindings, out_path: std::path::PathBuf) {
    let mut generated = Vec::new();
    bindings.write(Box::new(&mut generated)).expect("Couldn't render bindings!");

    // 生成文件仍放在 src/ 下供当前模块结构直接 include，但只有内容变化时才写回。
    // 这样 bindgen 每次运行不会单纯刷新 ignored 文件时间戳，避免 Cargo 把一串依赖 crate
    // 误判为需要重新编译；ABI 变化时内容不同，仍会正常写入并触发后续 rebuild。
    if std::fs::read(&out_path).is_ok_and(|old_content| old_content == generated) {
        return;
    }

    std::fs::write(out_path, generated).expect("Couldn't write bindings!");
}

/// 读取 Assimp C API 头文件，输出到当前 crate 中。
fn gen_rust_binding() {
    let cxx_root_path = TruvisPath::cxx_root_path();

    let bindings = bindgen::Builder::default()
        .header(cxx_root_path.join("mods/truvixx-assimp/include/TruvixxAssimp/c_api/module.h").to_str().unwrap())
        .clang_args([format!(
            "-I{}",
            cxx_root_path.join("mods/truvixx-assimp/include").to_str().unwrap()
        )])
        // 任何被包含的头文件变化时，都通知 cargo 重新构建当前 crate。
        .raw_line("#![allow(clippy::all)]")
        .raw_line("#![allow(warnings)]")
        .derive_default(true)
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .enable_cxx_namespaces()
        .generate()
        .expect("Unable to generate bindings");

    // 将 bindings 写入 crate 内的生成文件。
    let out_path = std::path::PathBuf::from("src").join("_ffi_bindings.rs");
    write_binding_if_changed(bindings, out_path);
}

/// 强制执行的方法: touch build.rs; cargo build
fn main() {
    let cxx_root_path = TruvisPath::cxx_root_path();

    println!("cargo:rerun-if-changed={}", cxx_root_path.join("CMakeLists.txt").display());
    println!("cargo:rerun-if-changed={}", cxx_root_path.join("vcpkg.json").display());
    println!("cargo:rerun-if-changed={}", cxx_root_path.join("mods/truvixx-assimp").display());
    println!("cargo:rerun-if-changed=build.rs");

    // 将自动绑定文件写入到当前项目中
    gen_rust_binding();

    let build_type = std::env::var("PROFILE").unwrap();

    let cargo_build_dir = TruvisPath::target_path().join(build_type);

    println!("cargo:rustc-link-search=native={}", cargo_build_dir.display());
    println!("cargo:rustc-link-lib=dylib=truvixx-assimp-capi");
}
