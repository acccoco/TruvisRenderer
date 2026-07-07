use std::{
    env, fs,
    path::{Path, PathBuf},
};

use truvis_path::TruvisPath;

fn binding_content_hash(content: &[u8]) -> String {
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET_BASIS;
    for byte in content {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }

    format!("{hash:016x}")[..8].to_owned()
}

fn write_file_if_changed(out_path: &Path, content: &[u8]) {
    if fs::read(out_path).is_ok_and(|old_content| old_content == content) {
        return;
    }

    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent).expect("Couldn't create binding output directory!");
    }

    fs::write(out_path, content).expect("Couldn't write bindings!");
}

fn write_binding_if_changed(bindings: bindgen::Bindings, out_path: PathBuf) -> (String, PathBuf) {
    let mut generated = Vec::new();
    bindings.write(Box::new(&mut generated)).expect("Couldn't render bindings!");
    let content_hash = binding_content_hash(&generated);
    let hash_path = out_path.with_extension("hash");

    // 生成文件位于 workspace 级 build/bindings 目录；只有内容变化时才写回，
    // 避免 bindgen 每次运行都刷新时间戳，减少下游 crate 的无意义 rebuild。
    write_file_if_changed(&out_path, &generated);
    write_file_if_changed(&hash_path, format!("{content_hash}\n").as_bytes());

    (content_hash, hash_path)
}

fn binding_output_path() -> PathBuf {
    let target = env::var("TARGET").expect("TARGET must be set by Cargo build scripts");
    TruvisPath::rust_binding_build_dir()
        .join(target)
        .join("cxx")
        .join("truvis-streamline-binding")
        .join("_ffi_bindings.rs")
}

/// 读取 Streamline C API 头文件，输出到 workspace 级 Rust binding 生成目录。
fn gen_rust_binding() -> (String, PathBuf) {
    let cxx_root_path = TruvisPath::cxx_root_path();
    let module_path = cxx_root_path.join("mods/truvixx-streamline");

    let bindings = bindgen::Builder::default()
        .header(module_path.join("include/TruvixxStreamline/c_api/module.h").to_str().unwrap())
        .clang_args([format!("-I{}", module_path.join("include").to_str().unwrap())])
        // 任何被包含的头文件变化时，都通知 cargo 重新构建当前 crate。
        .raw_line("#[allow(clippy::all)]")
        .raw_line("#[allow(warnings)]")
        .derive_default(true)
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .enable_cxx_namespaces()
        .generate()
        .expect("Unable to generate bindings");

    let out_path = binding_output_path();
    write_binding_if_changed(bindings, out_path)
}

fn main() {
    let cxx_root_path = TruvisPath::cxx_root_path();
    let module_path = cxx_root_path.join("mods/truvixx-streamline");
    let out_path = binding_output_path();

    println!("cargo:rerun-if-changed={}", cxx_root_path.join("CMakeLists.txt").display());
    println!("cargo:rerun-if-changed={}", cxx_root_path.join("vcpkg.json").display());
    println!("cargo:rerun-if-changed={}", module_path.display());
    println!("cargo:rerun-if-changed=build.rs");
    let (bindings_hash, bindings_hash_file) = gen_rust_binding();
    println!("cargo:rustc-env=TRUVIS_STREAMLINE_BINDINGS_RS={}", out_path.display());
    println!("cargo:rustc-env=TRUVIS_STREAMLINE_BINDINGS_HASH={bindings_hash}");
    println!("cargo:rustc-env=TRUVIS_STREAMLINE_BINDINGS_HASH_FILE={}", bindings_hash_file.display());

    let build_type = std::env::var("PROFILE").unwrap();
    let cargo_build_dir = TruvisPath::target_path().join(build_type);

    println!("cargo:rustc-link-search=native={}", cargo_build_dir.display());
    println!("cargo:rustc-link-lib=dylib=truvixx-streamline-capi");
}
