//! Shader Rust binding 的公共生成支持。
//!
//! 所有 binding crate 都通过本 crate 共享 bindgen 参数、Slang 基础类型重命名、
//! 固定输出路径和 write-if-changed 语义，避免不同 owner 生成出不一致的 Rust 投影。

use std::{
    env, fs,
    path::{Path, PathBuf},
};

use bindgen::callbacks::ItemInfo;
use truvis_path::TruvisPath;

/// 注入 bindgen 生成 namespace 的一行 Rust 代码。
pub struct ModuleRawLine<'a> {
    pub module: &'a str,
    pub line: &'a str,
}

/// 单个 shader binding owner 的生成契约。
pub struct BindingSpec<'a> {
    pub header: &'a Path,
    pub include_dirs: &'a [PathBuf],
    pub output_crate: &'a str,
    pub allowlist_types: &'a [&'a str],
    pub allowlist_vars: &'a [&'a str],
    pub module_raw_lines: &'a [ModuleRawLine<'a>],
    pub rerun_if_changed: &'a [PathBuf],
}

/// 生成文件及其内容 hash，供 binding crate 的 `build.rs` 暴露给 `include!`。
pub struct GeneratedBinding {
    pub source_path: PathBuf,
    pub hash_path: PathBuf,
    pub content_hash: String,
}

/// 绑定生成协调者。
pub struct BindingGenerator<'a> {
    spec: BindingSpec<'a>,
}

impl<'a> BindingGenerator<'a> {
    pub fn new(spec: BindingSpec<'a>) -> Self {
        assert!(
            !spec.allowlist_types.is_empty() || !spec.allowlist_vars.is_empty(),
            "Shader binding spec must provide at least one type or var allowlist"
        );
        Self { spec }
    }

    pub fn generate(self) -> GeneratedBinding {
        self.emit_rerun_inputs();

        let mut builder = bindgen::Builder::default()
            .header(self.spec.header.to_string_lossy())
            .clang_args(["-x", "c++", "-std=c++17"])
            .derive_default(false)
            .raw_line("#[allow(clippy::all)]")
            .raw_line("#[allow(warnings)]")
            .enable_cxx_namespaces()
            .allowlist_recursively(false)
            .parse_callbacks(Box::new(SlangTypeRenamer))
            .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()));

        for include_dir in self.spec.include_dirs {
            builder = builder.clang_arg(format!("-I{}", include_dir.display()));
        }
        for pattern in self.spec.allowlist_types {
            builder = builder.allowlist_type(pattern);
        }
        for pattern in self.spec.allowlist_vars {
            builder = builder.allowlist_var(pattern);
        }
        for raw_line in self.spec.module_raw_lines {
            builder = builder.module_raw_line(raw_line.module, raw_line.line);
        }

        let bindings = builder.generate().expect("Unable to generate shader bindings");
        let mut generated = Vec::new();
        bindings.write(Box::new(&mut generated)).expect("Couldn't render shader bindings");

        let source_path = self.output_path();
        let content_hash = Self::content_hash(&generated);
        let hash_path = source_path.with_extension("hash");

        // 生成文件位于 workspace 级 build/bindings 目录；只有内容变化时才写回，
        // 避免 bindgen 每次运行都刷新时间戳，减少下游 crate 的无意义 rebuild。
        Self::write_file_if_changed(&source_path, &generated);
        Self::write_file_if_changed(&hash_path, format!("{content_hash}\n").as_bytes());

        GeneratedBinding {
            source_path,
            hash_path,
            content_hash,
        }
    }

    fn output_path(&self) -> PathBuf {
        let target = env::var("TARGET").expect("TARGET must be set by Cargo build scripts");
        TruvisPath::rust_binding_build_dir()
            .join(target)
            .join("shader")
            .join(self.spec.output_crate)
            .join("_shader_bindings.rs")
    }

    fn emit_rerun_inputs(&self) {
        println!("cargo:rerun-if-changed={}", self.spec.header.display());
        for path in self.spec.rerun_if_changed {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }

    fn content_hash(content: &[u8]) -> String {
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
            fs::create_dir_all(parent).expect("Couldn't create shader binding output directory!");
        }

        fs::write(out_path, content).expect("Couldn't write shader bindings!");
    }
}

#[derive(Debug)]
struct SlangTypeRenamer;

impl bindgen::callbacks::ParseCallbacks for SlangTypeRenamer {
    fn item_name(&self, item: ItemInfo) -> Option<String> {
        match item.name {
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
            _ => None,
        }
    }
}
