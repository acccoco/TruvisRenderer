//! CXX public DLL 的 Rust binding 公共生成支持。
//!
//! Native module 不依赖本 crate；binding owner 的 `build.rs` 单向读取 C ABI header，
//! 并在这里复用 allowlist、稳定输出路径和 write-if-changed 行为。

use std::{
    fs,
    path::{Path, PathBuf},
};

use truvis_path::TruvisPath;

/// 在生成的 namespace 中显式复用由 binding owner 选择的 Rust 类型。
pub struct ModuleRawLine<'a> {
    pub module: &'a str,
    pub line: &'a str,
}

/// 单个 public CXX module 的 Rust binding 生成契约。
pub struct CxxBindingSpec<'a> {
    pub output_crate: &'a str,
    pub header: PathBuf,
    pub include_roots: Vec<PathBuf>,
    pub allowlist_types: &'a [&'a str],
    pub allowlist_functions: &'a [&'a str],
    pub allowlist_vars: &'a [&'a str],
    pub module_raw_lines: &'a [ModuleRawLine<'a>],
    pub layout_tests: bool,
}

/// 生成文件及其内容 hash，供 binding crate 的 `include!` 使用。
pub struct GeneratedCxxBinding {
    pub source_path: PathBuf,
    pub hash_path: PathBuf,
    pub content_hash: String,
}

/// CXX binding 生成协调者。
pub struct CxxBindingGenerator<'a> {
    spec: CxxBindingSpec<'a>,
}

impl<'a> CxxBindingGenerator<'a> {
    pub fn new(spec: CxxBindingSpec<'a>) -> Self {
        assert!(
            !spec.allowlist_types.is_empty() || !spec.allowlist_functions.is_empty() || !spec.allowlist_vars.is_empty(),
            "CXX binding spec must provide a non-empty allowlist"
        );
        assert!(
            !spec.output_crate.is_empty()
                && spec.output_crate.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')),
            "CXX binding output crate must be a canonical crate id"
        );
        Self { spec }
    }

    pub fn generate(self) -> GeneratedCxxBinding {
        self.emit_rerun_inputs();

        let mut builder = bindgen::Builder::default()
            .header(self.spec.header.to_string_lossy())
            .clang_args(["-x", "c++", "-std=c++20"])
            .derive_default(true)
            .layout_tests(self.spec.layout_tests)
            .raw_line("#[allow(clippy::all)]")
            .raw_line("#[allow(warnings)]")
            .enable_cxx_namespaces()
            .allowlist_recursively(false)
            .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()));

        for include_root in &self.spec.include_roots {
            builder = builder.clang_arg(format!("-I{}", include_root.display()));
        }
        for pattern in self.spec.allowlist_types {
            builder = builder.allowlist_type(pattern);
        }
        for pattern in self.spec.allowlist_functions {
            builder = builder.allowlist_function(pattern);
        }
        for pattern in self.spec.allowlist_vars {
            builder = builder.allowlist_var(pattern);
        }
        for raw_line in self.spec.module_raw_lines {
            builder = builder.module_raw_line(raw_line.module, raw_line.line);
        }

        let bindings = builder.generate().expect("Unable to generate CXX bindings");
        let mut generated = Vec::new();
        bindings.write(Box::new(&mut generated)).expect("Couldn't render CXX bindings");

        let target = std::env::var("TARGET").expect("TARGET must be set by Cargo build scripts");
        let source_path = TruvisPath::rust_binding_build_dir()
            .join(target)
            .join("cxx")
            .join(self.spec.output_crate)
            .join("_ffi_bindings.rs");
        let content_hash = Self::content_hash(&generated);
        let hash_path = source_path.with_extension("hash");

        Self::write_file_if_changed(&source_path, &generated);
        Self::write_file_if_changed(&hash_path, format!("{content_hash}\n").as_bytes());

        GeneratedCxxBinding {
            source_path,
            hash_path,
            content_hash,
        }
    }

    fn emit_rerun_inputs(&self) {
        println!("cargo:rerun-if-changed={}", self.spec.header.display());
        for include_root in &self.spec.include_roots {
            println!("cargo:rerun-if-changed={}", include_root.display());
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
            fs::create_dir_all(parent).expect("Couldn't create CXX binding output directory");
        }
        fs::write(out_path, content).expect("Couldn't write CXX bindings");
    }
}
