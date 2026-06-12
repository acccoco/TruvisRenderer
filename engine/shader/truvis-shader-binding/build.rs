use bindgen::callbacks::ItemInfo;
use truvis_path::TruvisPath;

fn write_binding_if_changed(bindings: bindgen::Bindings, out_path: std::path::PathBuf) {
    let mut generated = Vec::new();
    bindings.write(Box::new(&mut generated)).expect("Couldn't render bindings!");

    // 生成文件仍放在 src/ 下供当前模块结构直接 include，但只有内容变化时才写回。
    // 这样 bindgen 每次运行不会单纯刷新 ignored 文件时间戳，避免 Cargo 把 shader binding
    // 及其下游渲染 crate 误判为需要重新编译；共享结构变化时内容不同，仍会正常触发 rebuild。
    if std::fs::read(&out_path).is_ok_and(|old_content| old_content == generated) {
        return;
    }

    std::fs::write(out_path, generated).expect("Couldn't write bindings!");
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

    fn add_derives(&self, info: &bindgen::callbacks::DeriveInfo) -> Vec<String> {
        // 为结构体添加 Pod 和相关 traits
        if info.kind == bindgen::callbacks::TypeKind::Struct {
            vec![
                // "Clone".into(), //
                // "Copy".into(),  //
                // "bytemuck::Pod".into(),      //
                // "bytemuck::Zeroable".into(), //
            ]
        } else {
            vec![]
        }
    }
}

fn gen_rust_binding() {
    let shader_root_path = TruvisPath::shader_root_path();

    let bindings = bindgen::Builder::default()
        .header("./ffi/rust_ffi.hpp")
        .clang_arg(format!("-I{}", shader_root_path.to_str().unwrap()))
        .derive_default(false)
        // 禁用 clippy 的检查
        .raw_line("#![allow(clippy::all)]")
        .raw_line("#![allow(warnings)]")
        .enable_cxx_namespaces()
        // .ignore_functions()
        // 添加自定义回调
        .parse_callbacks(Box::new(ModifyAdder))
        // 同时保留 cargo 回调
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .generate()
        .expect("Unable to generate bindings");

    // 将 bindings 写入 crate 内的生成文件。
    let out_path = std::path::PathBuf::from("src").join("_shader_bindings.rs");
    write_binding_if_changed(bindings, out_path);
}

fn main() {
    gen_rust_binding();
}
