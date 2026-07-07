use std::{
    env, fs,
    path::{Path, PathBuf},
};

use bindgen::callbacks::ItemInfo;
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
        .join("shader")
        .join("truvis-shader-binding")
        .join("_shader_bindings.rs")
}

// 创建自定义回调实现
#[derive(Debug)]
struct ModifyAdder;
impl bindgen::callbacks::ParseCallbacks for ModifyAdder {
    fn item_name(&self, _original_name: ItemInfo) -> Option<String> {
        match _original_name.name {
            "uint" => Some("Uint".to_string()),
            "uint2" => Some("Uint2".to_string()),
            "uint3" => Some("Uint3".to_string()),
            "uint4" => Some("Uint4".to_string()),

            "int2" => Some("Int2".to_string()),
            "int3" => Some("Int3".to_string()),
            "int4" => Some("Int4".to_string()),

            "float2" => Some("Float2".to_string()),
            "float3" => Some("Float3".to_string()),
            "float4" => Some("Float4".to_string()),

            "float4x4" => Some("Float4x4".to_string()),

            &_ => None,
        }
    }
}

fn gen_rust_binding() -> (String, PathBuf) {
    let shader_root_path = TruvisPath::shader_root_path();

    let bindings = bindgen::Builder::default()
        .header("./ffi/rust_ffi.hpp")
        .clang_arg(format!("-I{}", shader_root_path.to_str().unwrap()))
        .derive_default(false)
        // 禁用 clippy 的检查
        .raw_line("#[allow(clippy::all)]")
        .raw_line("#[allow(warnings)]")
        .enable_cxx_namespaces()
        // .ignore_functions()
        // 添加自定义回调
        .parse_callbacks(Box::new(ModifyAdder))
        // 同时保留 cargo 回调
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .generate()
        .expect("Unable to generate bindings");

    let out_path = binding_output_path();
    write_binding_if_changed(bindings, out_path)
}

fn main() {
    let out_path = binding_output_path();
    let (bindings_hash, bindings_hash_file) = gen_rust_binding();

    println!("cargo:rustc-env=TRUVIS_SHADER_BINDINGS_RS={}", out_path.display());
    println!("cargo:rustc-env=TRUVIS_SHADER_BINDINGS_HASH={bindings_hash}");
    println!("cargo:rustc-env=TRUVIS_SHADER_BINDINGS_HASH_FILE={}", bindings_hash_file.display());
}
