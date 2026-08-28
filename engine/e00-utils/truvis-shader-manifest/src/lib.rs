//! Shader 构建配置的唯一解析入口。
//!
//! 路径都以 manifest 所在目录为根。该 crate 只验证 schema、词法路径和 package 图，
//! 不要求 shader 源码、编译器或输出目录已经存在，因此运行时路径查询也可以复用同一模型。

use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    fs,
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShaderManifest {
    #[serde(skip)]
    manifest_path: PathBuf,

    #[serde(skip)]
    root_dir: PathBuf,

    build: ShaderBuildConfig,
    compiler: ShaderCompilerConfig,

    #[serde(rename = "package")]
    packages: Vec<ShaderPackage>,

    #[serde(default, rename = "binding")]
    bindings: Vec<ShaderBinding>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ShaderBuildConfig {
    pub shader_output_root: String,
    pub binding_output_root: String,
    pub log_root: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ShaderCompilerConfig {
    pub slangc: String,
    pub glslc: String,
    pub dxc: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ShaderPackage {
    pub id: String,
    pub entry_root: String,

    #[serde(default)]
    pub include_roots: Vec<String>,

    #[serde(default)]
    pub shared_inputs: Vec<ShaderSharedInput>,

    #[serde(default)]
    pub depends_on: Vec<String>,

    pub output_prefix: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ShaderSharedInput {
    pub path: String,
    pub layer: ShaderSourceLayer,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ShaderSourceLayer {
    Abi,
    Lib,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ShaderBinding {
    pub id: String,
    pub package: String,
    pub header: String,

    #[serde(default)]
    pub extra_include_roots: Vec<String>,

    pub output_crate: String,
}

impl ShaderManifest {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let requested_path = path.as_ref();
        let manifest_path = if requested_path.is_absolute() {
            requested_path.to_path_buf()
        } else {
            std::env::current_dir().context("无法读取当前工作目录")?.join(requested_path)
        };
        let root_dir = manifest_path.parent().context("shader manifest 必须位于一个目录中")?.to_path_buf();
        let content = fs::read_to_string(&manifest_path)
            .with_context(|| format!("无法读取 shader manifest {}", manifest_path.display()))?;
        let mut manifest: Self = toml::from_str(&content)
            .with_context(|| format!("无法解析 shader manifest {}", manifest_path.display()))?;
        manifest.manifest_path = manifest_path;
        manifest.root_dir = root_dir;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn manifest_path(&self) -> &Path {
        &self.manifest_path
    }

    pub fn root_dir(&self) -> &Path {
        &self.root_dir
    }

    pub fn build(&self) -> &ShaderBuildConfig {
        &self.build
    }

    pub fn compiler(&self) -> &ShaderCompilerConfig {
        &self.compiler
    }

    pub fn packages(&self) -> &[ShaderPackage] {
        &self.packages
    }

    pub fn bindings(&self) -> &[ShaderBinding] {
        &self.bindings
    }

    pub fn package(&self, package_id: &str) -> Result<&ShaderPackage> {
        self.packages
            .iter()
            .find(|package| package.id == package_id)
            .with_context(|| format!("未知 shader package id: {package_id}"))
    }

    pub fn binding(&self, binding_id: &str) -> Result<&ShaderBinding> {
        self.bindings
            .iter()
            .find(|binding| binding.id == binding_id)
            .with_context(|| format!("未知 shader binding id: {binding_id}"))
    }

    pub fn resolve_path(&self, relative_path: &str) -> PathBuf {
        // Windows 下保留旧工具链传给编译器的路径形式，避免调试信息仅因分隔符变化而改变。
        #[cfg(windows)]
        let relative_path = relative_path.replace('/', "\\");

        self.root_dir.join(relative_path)
    }

    pub fn shader_output_root(&self) -> PathBuf {
        self.resolve_path(&self.build.shader_output_root)
    }

    pub fn binding_output_root(&self) -> PathBuf {
        self.resolve_path(&self.build.binding_output_root)
    }

    pub fn log_root(&self) -> PathBuf {
        self.resolve_path(&self.build.log_root)
    }

    pub fn slangc_executable(&self) -> PathBuf {
        self.resolve_executable(&self.compiler.slangc)
    }

    pub fn glslc_executable(&self) -> PathBuf {
        self.resolve_executable(&self.compiler.glslc)
    }

    pub fn dxc_executable(&self) -> PathBuf {
        self.resolve_executable(&self.compiler.dxc)
    }

    pub fn shader_output_path(&self, package_id: &str, entry_relative_path: &str) -> Result<PathBuf> {
        Self::validate_relative_path("shader entry relative path", entry_relative_path)?;
        let package = self.package(package_id)?;
        let path = self.shader_output_root().join(&package.output_prefix).join(entry_relative_path);
        let mut output = OsString::from(path.as_os_str());
        output.push(".spv");
        Ok(PathBuf::from(output))
    }

    pub fn binding_output_path(&self, target: &str, binding_id: &str) -> Result<PathBuf> {
        Self::validate_relative_path("shader binding target", target)?;
        let binding = self.binding(binding_id)?;
        Ok(self
            .binding_output_root()
            .join(target)
            .join("shader")
            .join(&binding.output_crate)
            .join("_shader_bindings.rs"))
    }

    pub fn package_dependency_closure(&self, package_id: &str) -> Result<Vec<&ShaderPackage>> {
        self.package(package_id)?;
        let mut collected = BTreeSet::new();
        self.collect_dependency_ids(package_id, &mut collected)?;
        collected.into_iter().map(|id| self.package(&id)).collect()
    }

    fn validate(&self) -> Result<()> {
        Self::validate_relative_path("build.shader_output_root", &self.build.shader_output_root)?;
        Self::validate_relative_path("build.binding_output_root", &self.build.binding_output_root)?;
        Self::validate_relative_path("build.log_root", &self.build.log_root)?;
        Self::validate_compiler("compiler.slangc", &self.compiler.slangc)?;
        Self::validate_compiler("compiler.glslc", &self.compiler.glslc)?;
        Self::validate_compiler("compiler.dxc", &self.compiler.dxc)?;

        if self.packages.is_empty() {
            bail!("shader-packages.toml 至少需要一个 [[package]]");
        }

        let mut package_ids = BTreeSet::new();
        let mut output_prefixes = BTreeSet::new();
        let mut shared_roots = BTreeSet::new();
        for package in &self.packages {
            Self::validate_id("shader package", &package.id)?;
            if !package_ids.insert(package.id.clone()) {
                bail!("重复 shader package id: '{}'", package.id);
            }
            Self::validate_relative_path("entry_root", &package.entry_root)?;
            Self::validate_relative_path("output_prefix", &package.output_prefix)?;
            if !output_prefixes.insert(package.output_prefix.clone()) {
                bail!("重复 shader output_prefix: '{}'", package.output_prefix);
            }
            if package.include_roots.is_empty() {
                bail!("shader package '{}' 至少需要一个 include_root", package.id);
            }

            let mut include_roots = BTreeSet::new();
            for path in &package.include_roots {
                Self::validate_relative_path("include_roots", path)?;
                if !include_roots.insert(path) {
                    bail!("shader package '{}' 存在重复 include_root: '{}'", package.id, path);
                }
            }
            for shared_input in &package.shared_inputs {
                Self::validate_relative_path("shared_inputs.path", &shared_input.path)?;
                if !shared_roots.insert(shared_input.path.clone()) {
                    bail!("shader shared input root 被重复声明: '{}'", shared_input.path);
                }
            }
        }

        let dependency_map = self
            .packages
            .iter()
            .map(|package| (package.id.as_str(), package.depends_on.as_slice()))
            .collect::<BTreeMap<_, _>>();
        for package in &self.packages {
            for dependency in &package.depends_on {
                if dependency == &package.id {
                    bail!("shader package '{}' 不能依赖自身", package.id);
                }
                if !dependency_map.contains_key(dependency.as_str()) {
                    bail!("shader package '{}' 依赖不存在的 package '{}'", package.id, dependency);
                }
            }
        }

        let mut visiting = BTreeSet::new();
        let mut visited = BTreeSet::new();
        for package in &self.packages {
            self.visit_dependencies(&package.id, &mut visiting, &mut visited)?;
        }
        self.validate_include_coverage()?;
        self.validate_bindings()?;
        Ok(())
    }

    fn validate_include_coverage(&self) -> Result<()> {
        for package in &self.packages {
            let visible_packages = self.package_dependency_closure(&package.id)?;
            for owner in visible_packages {
                for shared_input in &owner.shared_inputs {
                    let shared_path = self.resolve_path(&shared_input.path);
                    let matching_roots = package
                        .include_roots
                        .iter()
                        .map(|path| self.resolve_path(path))
                        .filter(|root| shared_path.starts_with(root))
                        .collect::<Vec<_>>();
                    if matching_roots.len() != 1 {
                        bail!(
                            "shader package '{}' 必须用唯一 include_root 覆盖 shared input '{}'",
                            package.id,
                            shared_input.path
                        );
                    }
                    let prefix = shared_path
                        .strip_prefix(&matching_roots[0])
                        .context("已匹配的 shared input 无法计算 include prefix")?;
                    if prefix.components().filter(|component| matches!(component, Component::Normal(_))).count() < 2 {
                        bail!(
                            "shader shared input '{}' 必须在 include_root 下保留 layer/owner 前缀",
                            shared_input.path
                        );
                    }
                }
            }
        }
        Ok(())
    }

    fn validate_bindings(&self) -> Result<()> {
        let mut ids = BTreeSet::new();
        let mut output_crates = BTreeSet::new();
        for binding in &self.bindings {
            Self::validate_id("shader binding", &binding.id)?;
            if !ids.insert(binding.id.clone()) {
                bail!("重复 shader binding id: '{}'", binding.id);
            }
            self.package(&binding.package)?;
            Self::validate_relative_path("binding.header", &binding.header)?;
            for path in &binding.extra_include_roots {
                Self::validate_relative_path("binding.extra_include_roots", path)?;
            }
            Self::validate_id("shader binding output_crate", &binding.output_crate)?;
            if !output_crates.insert(binding.output_crate.clone()) {
                bail!("shader binding output_crate 重复: '{}'", binding.output_crate);
            }
        }
        Ok(())
    }

    fn collect_dependency_ids(&self, package_id: &str, collected: &mut BTreeSet<String>) -> Result<()> {
        if !collected.insert(package_id.to_string()) {
            return Ok(());
        }
        for dependency in &self.package(package_id)?.depends_on {
            self.collect_dependency_ids(dependency, collected)?;
        }
        Ok(())
    }

    fn visit_dependencies(
        &self,
        package_id: &str,
        visiting: &mut BTreeSet<String>,
        visited: &mut BTreeSet<String>,
    ) -> Result<()> {
        if visited.contains(package_id) {
            return Ok(());
        }
        if !visiting.insert(package_id.to_string()) {
            bail!("shader package 依赖存在环，回到 '{package_id}'");
        }
        for dependency in &self.package(package_id)?.depends_on {
            self.visit_dependencies(dependency, visiting, visited)?;
        }
        visiting.remove(package_id);
        visited.insert(package_id.to_string());
        Ok(())
    }

    fn resolve_executable(&self, value: &str) -> PathBuf {
        let path = Path::new(value);
        if path.is_absolute() || (!value.contains('/') && !value.contains('\\')) {
            path.to_path_buf()
        } else {
            self.resolve_path(value)
        }
    }

    fn validate_id(label: &str, value: &str) -> Result<()> {
        if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')) {
            bail!("非法 {label} id: '{value}'");
        }
        Ok(())
    }

    fn validate_compiler(label: &str, value: &str) -> Result<()> {
        if value.trim().is_empty() {
            bail!("{label} 不能为空");
        }
        Ok(())
    }

    fn validate_relative_path(field: &str, value: &str) -> Result<()> {
        if value.is_empty() {
            bail!("{field} 不能为空");
        }
        let path = Path::new(value);
        if path.components().any(|component| {
            matches!(component, Component::CurDir | Component::ParentDir | Component::RootDir | Component::Prefix(_))
        }) {
            bail!("{field} 必须是 manifest 根内的 canonical 相对路径: '{value}'");
        }
        Ok(())
    }
}
