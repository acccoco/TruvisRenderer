use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

use truvis_path::TruvisPath;

/// binding 显式消费外层 CMake 已构建的文件；这里不启动构建、不发现依赖，也不管理历史部署。
/// DLL/JSON 复制是本地 workspace 约定：应用位于 profile 根，测试位于 deps；失败后需重新构建。
pub struct NativeLibrary {
    output_dir: PathBuf,
    vcpkg_bin_dir: PathBuf,
    cargo_dir: PathBuf,
    debug: bool,
}

impl NativeLibrary {
    pub fn new() -> Self {
        assert_eq!(
            std::env::var("TARGET").as_deref(),
            Ok("x86_64-pc-windows-msvc"),
            "CXX binding requires Windows x64 MSVC ABI"
        );
        println!("cargo:rerun-if-env-changed=TRUVIS_CXX_TOOL");
        let tool = std::env::var_os("TRUVIS_CXX_TOOL").unwrap_or_else(|| "clang-cl".into());
        let tool = tool.to_str().expect("TRUVIS_CXX_TOOL must be UTF-8");
        assert!(matches!(tool, "vs2022" | "vs2026" | "clang-cl"), "Unsupported TRUVIS_CXX_TOOL: {tool}");
        let profile = std::env::var("PROFILE").expect("PROFILE must be set by Cargo");
        let configuration = match profile.as_str() {
            "debug" => "Debug",
            "release" => "Release",
            _ => panic!("Unsupported CXX profile: {profile}"),
        };
        let root = TruvisPath::target().join("cxx");
        let mut cmake_dir = root.join("cmake").join(tool);
        if tool == "clang-cl" {
            cmake_dir.push(configuration);
        }
        let debug = profile == "debug";
        Self {
            output_dir: root.join("output").join(tool).join(configuration),
            vcpkg_bin_dir: cmake_dir.join("vcpkg_installed/x64-windows").join(if debug { "debug/bin" } else { "bin" }),
            cargo_dir: TruvisPath::target().join(profile),
            debug,
        }
    }

    pub fn is_debug(&self) -> bool {
        self.debug
    }

    pub fn output_dir(&self) -> &Path {
        &self.output_dir
    }

    pub fn vcpkg_bin_dir(&self) -> &Path {
        &self.vcpkg_bin_dir
    }

    /// import library 直接从 native 输出链接，不复制，也不从旧 Cargo 部署目录补找。
    pub fn link(&self, name: &str) {
        let library = self.output_dir.join(format!("{name}.lib"));
        Self::require_file(&library);
        println!("cargo:rustc-link-search=native={}", self.output_dir.display());
        println!("cargo:rustc-link-lib=dylib={name}");
    }

    pub fn copy_runtime(&self, directory: &Path, names: &[&str]) {
        for name in names {
            self.copy_file(&directory.join(name));
        }
    }

    pub fn copy_file(&self, source: &Path) {
        Self::require_file(source);
        for directory in [self.cargo_dir.clone(), self.cargo_dir.join("deps")] {
            fs::create_dir_all(&directory).expect("Cannot create Cargo runtime directory");
            let destination = directory.join(source.file_name().expect("Runtime file must have a name"));
            let result = (|| -> std::io::Result<()> {
                if !Self::same_content(source, &destination)? {
                    fs::copy(source, &destination)?;
                }
                Ok(())
            })();
            result.unwrap_or_else(|error| {
                panic!("Cannot copy {} to {}: {error}", source.display(), destination.display())
            });
        }
    }

    fn require_file(path: &Path) {
        println!("cargo:rerun-if-changed={}", path.display());
        assert!(
            path.is_file(),
            "Required native file missing: {}; build the selected CMake preset first",
            path.display()
        );
    }

    /// 内容相同时不覆盖正在使用的 DLL；时间戳不能判断切换工具链后的文件身份。
    fn same_content(source: &Path, destination: &Path) -> std::io::Result<bool> {
        let mut target = match fs::File::open(destination) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error),
        };
        let mut input = fs::File::open(source)?;
        let mut remaining = input.metadata()?.len();
        if remaining != target.metadata()?.len() {
            return Ok(false);
        }
        let mut source_buffer = [0; 64 * 1024];
        let mut target_buffer = [0; 64 * 1024];
        while remaining > 0 {
            let count = remaining.min(source_buffer.len() as u64) as usize;
            input.read_exact(&mut source_buffer[..count])?;
            target.read_exact(&mut target_buffer[..count])?;
            if source_buffer[..count] != target_buffer[..count] {
                return Ok(false);
            }
            remaining -= count as u64;
        }
        Ok(true)
    }
}
