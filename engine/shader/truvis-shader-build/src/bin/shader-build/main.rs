//! Shader 编译工具
//!
//! 按 `shader-packages.toml` 发现多个 shader package，将入口编译到带 package prefix 的
//! SPIR-V 目录，并维护 package-aware 增量 manifest。

mod common;
mod dependency;
mod glsl;
mod hlsl;
mod slang;

use std::{
    collections::{BTreeMap, BTreeSet},
    io::ErrorKind,
    path::{Component, Path, PathBuf},
    time::UNIX_EPOCH,
};

use common::{EnvPath, ShaderCompileTask, ShaderCompiler, ShaderCompilerType};
use dependency::{ShaderDependencyValidator, ShaderSourceLayer, ShaderSourcePackage, ShaderSourceRoot};
use glsl::GlslCompiler;
use hlsl::HlslCompiler;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use slang::SlangCompiler;
use truvis_logs::{LogFilePath, TruvisLogger};
use truvis_path::TruvisPath;

const MANIFEST_VERSION: u32 = 4;
const COMPILER_ARGS_VERSION: &str = "shader-compiler-args-v4-depfile";

/// 命令行只暴露最小控制面。
struct CliOptions {
    force: bool,
}

impl CliOptions {
    fn parse() -> Result<Self, String> {
        let mut force = false;
        for arg in std::env::args().skip(1) {
            match arg.as_str() {
                "--force" | "-f" => force = true,
                "--help" | "-h" => return Err("Usage: shader-build [--force]".to_string()),
                _ => return Err(format!("Unsupported shader-build arg '{arg}'")),
            }
        }

        Ok(Self { force })
    }
}

/// shader 编译流程的路径上下文。
struct ShaderBuildLayout {
    workspace_dir: PathBuf,
    output_dir: PathBuf,
    dependency_dir: PathBuf,
    state_manifest_path: PathBuf,
    package_manifest_path: PathBuf,
}

impl ShaderBuildLayout {
    fn new() -> Self {
        let workspace_dir = TruvisPath::workspace_path();
        let output_dir = EnvPath::shader_build_path().to_path_buf();
        Self {
            dependency_dir: output_dir.join(".deps"),
            state_manifest_path: output_dir.join(".state").join("shader-build.json"),
            package_manifest_path: workspace_dir.join("shader-packages.toml"),
            workspace_dir,
            output_dir,
        }
    }

    fn resolve_workspace_path(&self, relative_path: &str) -> PathBuf {
        self.workspace_dir.join(relative_path.replace('/', "\\"))
    }

    fn relative_slash_path(&self, path: &Path) -> String {
        path.strip_prefix(&self.workspace_dir).unwrap_or(path).to_string_lossy().replace('\\', "/")
    }

    /// 旧 manifest 只能删除本工具输出目录内的文件。
    fn managed_output_from_manifest(&self, relative_path: &str) -> Result<PathBuf, String> {
        let path = self.resolve_workspace_path(relative_path);
        if !path.starts_with(&self.output_dir) {
            return Err(format!("拒绝删除 shader 输出目录外的 manifest 路径: {}", path.display()));
        }
        Ok(path)
    }
}

#[derive(Clone, Debug, Deserialize)]
struct ShaderPackageFile {
    package: Vec<ShaderPackageConfig>,
}

/// 一个 shader source/binary owner 的声明。
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
struct ShaderPackageConfig {
    id: String,
    entry_root: String,
    #[serde(default)]
    include_roots: Vec<String>,
    #[serde(default)]
    shared_input_roots: Vec<String>,
    #[serde(default)]
    depends_on: Vec<String>,
    output_prefix: String,
}

/// 已校验的 package 集合。
struct ShaderPackageSet {
    packages: Vec<ShaderPackageConfig>,
}

impl ShaderPackageSet {
    fn load(layout: &ShaderBuildLayout) -> Result<Self, String> {
        let content = std::fs::read_to_string(&layout.package_manifest_path).map_err(|err| {
            format!("无法读取 shader package manifest {}: {err}", layout.package_manifest_path.display())
        })?;
        let mut file: ShaderPackageFile = toml::from_str(&content).map_err(|err| {
            format!("无法解析 shader package manifest {}: {err}", layout.package_manifest_path.display())
        })?;
        file.package.sort_by(|left, right| left.id.cmp(&right.id));

        let package_set = Self { packages: file.package };
        package_set.validate(layout)?;
        Ok(package_set)
    }

    fn validate(&self, layout: &ShaderBuildLayout) -> Result<(), String> {
        if self.packages.is_empty() {
            return Err("shader-packages.toml 至少需要一个 [[package]]".to_string());
        }

        let mut ids = BTreeSet::new();
        let mut output_prefixes = BTreeSet::new();
        for package in &self.packages {
            if package.id.is_empty()
                || !package.id.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            {
                return Err(format!("非法 shader package id: '{}'", package.id));
            }
            if !ids.insert(package.id.clone()) {
                return Err(format!("重复 shader package id: '{}'", package.id));
            }

            Self::validate_relative_path("entry_root", &package.entry_root)?;
            Self::validate_relative_path("output_prefix", &package.output_prefix)?;
            for path in &package.include_roots {
                Self::validate_relative_path("include_roots", path)?;
            }
            for path in &package.shared_input_roots {
                Self::validate_relative_path("shared_input_roots", path)?;
            }

            if !output_prefixes.insert(package.output_prefix.clone()) {
                return Err(format!("重复 shader output_prefix: '{}'", package.output_prefix));
            }

            Self::require_directory(layout, &package.entry_root, &package.id)?;
            for path in package.include_roots.iter().chain(&package.shared_input_roots) {
                Self::require_directory(layout, path, &package.id)?;
            }
        }

        let dependency_map = self
            .packages
            .iter()
            .map(|package| (package.id.clone(), package.depends_on.clone()))
            .collect::<BTreeMap<_, _>>();
        for package in &self.packages {
            for dependency in &package.depends_on {
                if dependency == &package.id {
                    return Err(format!("shader package '{}' 不能依赖自身", package.id));
                }
                if !dependency_map.contains_key(dependency) {
                    return Err(format!("shader package '{}' 依赖不存在的 package '{}'", package.id, dependency));
                }
            }
        }

        let mut visiting = BTreeSet::new();
        let mut visited = BTreeSet::new();
        for package in &self.packages {
            Self::visit_dependencies(&package.id, &dependency_map, &mut visiting, &mut visited)?;
        }

        Ok(())
    }

    fn validate_relative_path(field: &str, value: &str) -> Result<(), String> {
        if value.is_empty() {
            return Err(format!("{field} 不能为空"));
        }

        let path = Path::new(value);
        if path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::RootDir | Component::Prefix(_)))
        {
            return Err(format!("{field} 必须是 workspace 内的词法相对路径: '{value}'"));
        }
        Ok(())
    }

    fn require_directory(layout: &ShaderBuildLayout, relative_path: &str, package_id: &str) -> Result<(), String> {
        let path = layout.resolve_workspace_path(relative_path);
        if !path.is_dir() {
            return Err(format!("shader package '{}' 的目录不存在: {}", package_id, path.display()));
        }
        Ok(())
    }

    fn visit_dependencies(
        package_id: &str,
        dependency_map: &BTreeMap<String, Vec<String>>,
        visiting: &mut BTreeSet<String>,
        visited: &mut BTreeSet<String>,
    ) -> Result<(), String> {
        if visited.contains(package_id) {
            return Ok(());
        }
        if !visiting.insert(package_id.to_string()) {
            return Err(format!("shader package 依赖存在环，回到 '{package_id}'"));
        }

        for dependency in dependency_map.get(package_id).into_iter().flatten() {
            Self::visit_dependencies(dependency, dependency_map, visiting, visited)?;
        }

        visiting.remove(package_id);
        visited.insert(package_id.to_string());
        Ok(())
    }

    fn expand_changed_dependents(&self, directly_changed: &BTreeSet<String>) -> BTreeSet<String> {
        let mut changed = directly_changed.clone();
        loop {
            let mut added = false;
            for package in &self.packages {
                if changed.contains(&package.id) {
                    continue;
                }
                if package.depends_on.iter().any(|dependency| changed.contains(dependency)) {
                    changed.insert(package.id.clone());
                    added = true;
                }
            }
            if !added {
                return changed;
            }
        }
    }

    /// 当前 package 与其传递依赖只暴露 shared inputs，不暴露任何其它 entry。
    fn allowed_dependency_roots(&self, package: &ShaderPackageConfig, layout: &ShaderBuildLayout) -> Vec<PathBuf> {
        let mut package_ids = BTreeSet::from([package.id.clone()]);
        self.collect_dependency_ids(&package.id, &mut package_ids);

        let mut roots = package_ids
            .iter()
            .flat_map(|package_id| {
                self.packages
                    .iter()
                    .find(|candidate| &candidate.id == package_id)
                    .expect("validated shader package disappeared")
                    .shared_input_roots
                    .iter()
            })
            .map(|path| layout.resolve_workspace_path(path))
            .collect::<Vec<_>>();
        roots.sort();
        roots.dedup();
        roots
    }

    fn collect_dependency_ids(&self, package_id: &str, collected: &mut BTreeSet<String>) {
        let package = self
            .packages
            .iter()
            .find(|candidate| candidate.id == package_id)
            .expect("validated shader package disappeared");
        for dependency in &package.depends_on {
            if collected.insert(dependency.clone()) {
                self.collect_dependency_ids(dependency, collected);
            }
        }
    }
}

/// package-aware shader 构建协调者。
struct ShaderBuildRunner {
    layout: ShaderBuildLayout,
    packages: ShaderPackageSet,
    dependency_validator: ShaderDependencyValidator,
    force: bool,
}

impl ShaderBuildRunner {
    fn new(force: bool) -> Result<Self, String> {
        let layout = ShaderBuildLayout::new();
        let packages = ShaderPackageSet::load(&layout)?;
        let dependency_validator = ShaderDependencyValidator::new(&layout.workspace_dir)?;
        Ok(Self {
            layout,
            packages,
            dependency_validator,
            force,
        })
    }

    fn run(&self) -> Result<(), String> {
        log::info!("Shader package manifest: {:?}", self.layout.package_manifest_path);
        log::info!("Shader output path: {:?}", self.layout.output_dir);
        for package in &self.packages.packages {
            log::info!(
                "Shader package: id={} entry={} output={}",
                package.id,
                package.entry_root,
                package.output_prefix
            );
        }

        self.validate_source_dependencies()?;

        let previous_state = LoadedShaderBuildManifest::load(&self.layout.state_manifest_path)?;
        let previous_manifest = previous_state.current.as_ref();
        let package_states = self.collect_package_states()?;
        let directly_changed = self.directly_changed_packages(previous_manifest, &package_states);
        let changed_packages = self.packages.expand_changed_dependents(&directly_changed);

        let tasks = self.collect_tasks();
        self.remove_stale_outputs(&previous_state.managed_outputs, &tasks)?;
        self.remove_stale_depfiles(&tasks)?;
        self.prune_empty_output_directories()?;

        let previous_tasks = previous_manifest.map(ShaderBuildManifest::task_map).unwrap_or_default();
        let total_task_count = tasks.len();
        let mut next_task_manifests = Vec::with_capacity(total_task_count);
        let mut tasks_to_compile = Vec::new();

        for task in tasks {
            let task_manifest = ShaderTaskManifest::from_task(&self.layout, &task)?;
            let previous_task = previous_tasks.get(&task_manifest.task_id);
            let output_path = self.layout.resolve_workspace_path(&task_manifest.output_path);
            let needs_compile = self.force
                || changed_packages.contains(&task.package_id)
                || previous_task.is_none_or(|old_task| old_task != &task_manifest)
                || !output_path.is_file()
                || !task.depfile_path.is_file();

            if needs_compile {
                tasks_to_compile.push(task);
            }
            next_task_manifests.push(task_manifest);
        }

        self.compile_tasks(&tasks_to_compile)?;

        ShaderBuildManifest {
            version: MANIFEST_VERSION,
            packages: package_states,
            tasks: next_task_manifests,
        }
        .save(&self.layout.state_manifest_path)?;

        log::info!(
            "Shader compilation completed. compiled={}, skipped={}",
            tasks_to_compile.len(),
            total_task_count.saturating_sub(tasks_to_compile.len())
        );
        Ok(())
    }

    fn collect_tasks(&self) -> Vec<ShaderCompileTask> {
        let mut tasks = Vec::new();
        for package in &self.packages.packages {
            let entry_root = self.layout.resolve_workspace_path(&package.entry_root);
            let include_roots =
                package.include_roots.iter().map(|path| self.layout.resolve_workspace_path(path)).collect::<Vec<_>>();
            let allowed_dependency_roots = self.packages.allowed_dependency_roots(package, &self.layout);
            let output_prefix = Path::new(&package.output_prefix);

            tasks.extend(
                walkdir::WalkDir::new(&entry_root)
                    .into_iter()
                    .filter_map(Result::ok)
                    .filter(|entry| entry.path().is_file())
                    .filter_map(|entry| {
                        ShaderCompileTask::new(
                            &package.id,
                            &entry_root,
                            &self.layout.output_dir,
                            &self.layout.dependency_dir,
                            output_prefix,
                            &include_roots,
                            &allowed_dependency_roots,
                            &entry,
                        )
                    }),
            );
        }

        tasks.sort_by(|left, right| {
            (&left.package_id, &left.entry_relative_path).cmp(&(&right.package_id, &right.entry_relative_path))
        });
        tasks
    }

    fn validate_source_dependencies(&self) -> Result<(), String> {
        let packages = self
            .packages
            .packages
            .iter()
            .map(|package| {
                let mut source_roots = package
                    .shared_input_roots
                    .iter()
                    .map(|path| {
                        let layer = if path.contains("/abi/") {
                            ShaderSourceLayer::Abi
                        } else if path.contains("/lib/") {
                            ShaderSourceLayer::Lib
                        } else {
                            return Err(format!(
                                "shader package '{}' 的 shared_input_root 必须属于 abi 或 lib: '{}'",
                                package.id, path
                            ));
                        };
                        Ok(ShaderSourceRoot {
                            path: self.layout.resolve_workspace_path(path),
                            layer,
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                source_roots.push(ShaderSourceRoot {
                    path: self.layout.resolve_workspace_path(&package.entry_root),
                    layer: ShaderSourceLayer::Entry,
                });

                Ok(ShaderSourcePackage {
                    package_id: package.id.clone(),
                    include_roots: package
                        .include_roots
                        .iter()
                        .map(|path| self.layout.resolve_workspace_path(path))
                        .collect(),
                    source_roots,
                    allowed_dependency_roots: self.packages.allowed_dependency_roots(package, &self.layout),
                })
            })
            .collect::<Result<Vec<_>, String>>()?;

        self.dependency_validator.validate_sources(&packages)
    }

    fn collect_package_states(&self) -> Result<Vec<ShaderPackageState>, String> {
        let mut states = Vec::with_capacity(self.packages.packages.len());
        for package in &self.packages.packages {
            states.push(ShaderPackageState {
                config: package.clone(),
                shared_inputs: self.collect_shared_inputs(package)?,
            });
        }
        states.sort_by(|left, right| left.config.id.cmp(&right.config.id));
        Ok(states)
    }

    /// package 的本地 ABI/lib/include 变化只直接失效该 package；依赖传播在下一步统一完成。
    fn collect_shared_inputs(&self, package: &ShaderPackageConfig) -> Result<Vec<FileStamp>, String> {
        let entry_root = self.layout.resolve_workspace_path(&package.entry_root);
        let include_roots =
            package.include_roots.iter().map(|path| self.layout.resolve_workspace_path(path)).collect::<Vec<_>>();
        let allowed_dependency_roots = self.packages.allowed_dependency_roots(package, &self.layout);
        let output_prefix = Path::new(&package.output_prefix);
        let mut inputs = BTreeMap::new();

        for root in package
            .shared_input_roots
            .iter()
            .map(|path| self.layout.resolve_workspace_path(path))
            .chain(std::iter::once(entry_root.clone()))
        {
            for entry in walkdir::WalkDir::new(&root).into_iter().filter_map(Result::ok) {
                if !entry.path().is_file() {
                    continue;
                }

                if entry.path().starts_with(&entry_root)
                    && ShaderCompileTask::new(
                        &package.id,
                        &entry_root,
                        &self.layout.output_dir,
                        &self.layout.dependency_dir,
                        output_prefix,
                        &include_roots,
                        &allowed_dependency_roots,
                        &entry,
                    )
                    .is_some()
                {
                    continue;
                }

                let stamp = FileStamp::from_path(&self.layout, entry.path())?;
                inputs.insert(stamp.path.clone(), stamp);
            }
        }

        Ok(inputs.into_values().collect())
    }

    fn directly_changed_packages(
        &self,
        previous_manifest: Option<&ShaderBuildManifest>,
        current_states: &[ShaderPackageState],
    ) -> BTreeSet<String> {
        let Some(previous_manifest) = previous_manifest else {
            return current_states.iter().map(|state| state.config.id.clone()).collect();
        };
        let previous_states = previous_manifest.package_map();

        current_states
            .iter()
            .filter(|state| previous_states.get(&state.config.id).is_none_or(|old_state| *old_state != *state))
            .map(|state| state.config.id.clone())
            .collect()
    }

    fn compile_tasks(&self, tasks: &[ShaderCompileTask]) -> Result<(), String> {
        if tasks.is_empty() {
            log::info!("Shader inputs unchanged; skip shader compiler invocations.");
            return Ok(());
        }

        let errors = tasks
            .par_iter()
            .filter_map(|task| {
                log::info!("Compiling shader: package={} source={:?}", task.package_id, task.shader_path);
                self.compile_task(task).err()
            })
            .collect::<Vec<_>>();

        if errors.is_empty() { Ok(()) } else { Err(errors.join("\n")) }
    }

    fn compile_task(&self, task: &ShaderCompileTask) -> Result<(), String> {
        if let Some(parent) = task.output_path.parent()
            && let Err(err) = std::fs::create_dir_all(parent)
        {
            return Err(format!("无法创建 shader 输出目录 {}: {err}", parent.display()));
        }

        if let Some(parent) = task.depfile_path.parent()
            && let Err(err) = std::fs::create_dir_all(parent)
        {
            return Err(format!("无法创建 shader depfile 目录 {}: {err}", parent.display()));
        }
        if task.depfile_path.is_file() {
            std::fs::remove_file(&task.depfile_path)
                .map_err(|err| format!("无法删除旧 shader depfile {}: {err}", task.depfile_path.display()))?;
        }

        Self::compiler_for(task.compiler_type).compile(task)?;
        if let Err(err) = self.dependency_validator.validate_compiler_dependencies(task) {
            if task.output_path.is_file()
                && let Err(remove_err) = std::fs::remove_file(&task.output_path)
            {
                return Err(format!(
                    "{err}\n同时无法删除依赖越界的 shader 输出 {}: {remove_err}",
                    task.output_path.display()
                ));
            }
            return Err(err);
        }
        Ok(())
    }

    /// stale output 只按旧 manifest 声明删除，并额外限制在 build/shader 下。
    fn remove_stale_outputs(
        &self,
        previous_outputs: &[String],
        current_tasks: &[ShaderCompileTask],
    ) -> Result<(), String> {
        let current_outputs = current_tasks
            .iter()
            .map(|task| self.layout.relative_slash_path(&task.output_path))
            .collect::<BTreeSet<_>>();

        for old_output in previous_outputs {
            if current_outputs.contains(old_output) {
                continue;
            }

            let output_path = self.layout.managed_output_from_manifest(old_output)?;
            if output_path.is_file() {
                std::fs::remove_file(&output_path)
                    .map_err(|err| format!("无法删除旧 shader 输出 {}: {err}", output_path.display()))?;
                log::info!("Removed stale shader output: {}", output_path.display());
            }
        }

        Ok(())
    }

    /// `.deps` 完全由本工具管理；不再对应当前 task 的依赖产物可以安全删除。
    fn remove_stale_depfiles(&self, current_tasks: &[ShaderCompileTask]) -> Result<(), String> {
        if !self.layout.dependency_dir.is_dir() {
            return Ok(());
        }
        let current_depfiles = current_tasks.iter().map(|task| task.depfile_path.clone()).collect::<BTreeSet<_>>();
        for entry in walkdir::WalkDir::new(&self.layout.dependency_dir).into_iter().filter_map(Result::ok) {
            let path = entry.path();
            if path.is_file() && !current_depfiles.contains(path) {
                std::fs::remove_file(path)
                    .map_err(|err| format!("无法删除旧 shader depfile {}: {err}", path.display()))?;
                log::info!("Removed stale shader dependency artifact: {}", path.display());
            }
        }
        Ok(())
    }

    /// build/shader 是工具管理目录；只删除空目录，并永远停在输出根目录以内。
    fn prune_empty_output_directories(&self) -> Result<(), String> {
        if !self.layout.output_dir.is_dir() {
            return Ok(());
        }
        for entry in walkdir::WalkDir::new(&self.layout.output_dir)
            .min_depth(1)
            .contents_first(true)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|entry| entry.path().is_dir())
        {
            let path = entry.path();
            if !path.starts_with(&self.layout.output_dir) {
                return Err(format!("拒绝清理 shader 输出目录外的空目录: {}", path.display()));
            }
            match std::fs::remove_dir(path) {
                Ok(()) => log::info!("Removed empty shader output directory: {}", path.display()),
                Err(err) if matches!(err.kind(), ErrorKind::DirectoryNotEmpty | ErrorKind::NotFound) => {}
                Err(err) => return Err(format!("无法清理空 shader 输出目录 {}: {err}", path.display())),
            }
        }
        Ok(())
    }

    fn compiler_for(compiler_type: ShaderCompilerType) -> Box<dyn ShaderCompiler> {
        match compiler_type {
            ShaderCompilerType::Glsl => Box::new(GlslCompiler::new()),
            ShaderCompilerType::Hlsl => Box::new(HlslCompiler::new()),
            ShaderCompilerType::Slang => Box::new(SlangCompiler::new()),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct ShaderBuildManifest {
    version: u32,
    packages: Vec<ShaderPackageState>,
    tasks: Vec<ShaderTaskManifest>,
}

impl ShaderBuildManifest {
    fn save(&self, path: &Path) -> Result<(), String> {
        let content = serde_json::to_string_pretty(self)
            .map_err(|err| format!("无法序列化 shader manifest {}: {err}", path.display()))?;
        Self::write_if_changed(path, content.as_bytes())
    }

    fn package_map(&self) -> BTreeMap<String, &ShaderPackageState> {
        self.packages.iter().map(|state| (state.config.id.clone(), state)).collect()
    }

    fn task_map(&self) -> BTreeMap<String, ShaderTaskManifest> {
        self.tasks.iter().map(|task| (task.task_id.clone(), task.clone())).collect()
    }

    fn write_if_changed(path: &Path, content: &[u8]) -> Result<(), String> {
        if path.is_file() {
            let old_content =
                std::fs::read(path).map_err(|err| format!("无法读取已有文件 {}: {err}", path.display()))?;
            if old_content == content {
                return Ok(());
            }
        }

        let parent = path.parent().ok_or_else(|| format!("无法获取目标目录: {}", path.display()))?;
        std::fs::create_dir_all(parent).map_err(|err| format!("无法创建目录 {}: {err}", parent.display()))?;
        std::fs::write(path, content).map_err(|err| format!("无法写入文件 {}: {err}", path.display()))
    }
}

/// 版本变化时仍保留旧 manifest 声明过的输出集合，确保目录重排不会遗留幽灵 SPIR-V。
struct LoadedShaderBuildManifest {
    current: Option<ShaderBuildManifest>,
    managed_outputs: Vec<String>,
}

impl LoadedShaderBuildManifest {
    fn load(path: &Path) -> Result<Self, String> {
        if !path.is_file() {
            return Ok(Self {
                current: None,
                managed_outputs: Vec::new(),
            });
        }

        let content = std::fs::read_to_string(path)
            .map_err(|err| format!("无法读取 shader manifest {}: {err}", path.display()))?;
        let value: serde_json::Value = serde_json::from_str(&content)
            .map_err(|err| format!("无法解析 shader manifest {}: {err}", path.display()))?;
        let managed_outputs = value
            .get("tasks")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|task| task.get("output_path").and_then(serde_json::Value::as_str))
            .map(str::to_string)
            .collect::<Vec<_>>();
        let version = value.get("version").and_then(serde_json::Value::as_u64);
        let current = if version == Some(u64::from(MANIFEST_VERSION)) {
            Some(
                serde_json::from_value(value)
                    .map_err(|err| format!("无法解析当前 shader manifest {}: {err}", path.display()))?,
            )
        } else {
            None
        };

        Ok(Self {
            current,
            managed_outputs,
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct ShaderPackageState {
    config: ShaderPackageConfig,
    shared_inputs: Vec<FileStamp>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct ShaderTaskManifest {
    task_id: String,
    package_id: String,
    entry_relative_path: String,
    shader_path: String,
    output_path: String,
    dependency_path: String,
    shader_input: FileStamp,
    shader_stage: String,
    compiler_type: String,
    compiler_args_version: String,
}

impl ShaderTaskManifest {
    fn from_task(layout: &ShaderBuildLayout, task: &ShaderCompileTask) -> Result<Self, String> {
        let entry_relative_path = task.entry_relative_path.to_string_lossy().replace('\\', "/");
        Ok(Self {
            task_id: format!("{}/{}", task.package_id, entry_relative_path),
            package_id: task.package_id.clone(),
            entry_relative_path,
            shader_path: layout.relative_slash_path(&task.shader_path),
            output_path: layout.relative_slash_path(&task.output_path),
            dependency_path: layout.relative_slash_path(&task.depfile_path),
            shader_input: FileStamp::from_path(layout, &task.shader_path)?,
            shader_stage: format!("{:?}", task.shader_stage),
            compiler_type: format!("{:?}", task.compiler_type),
            compiler_args_version: COMPILER_ARGS_VERSION.to_string(),
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
struct FileStamp {
    path: String,
    len: u64,
    modified_ms: u64,
}

impl FileStamp {
    fn from_path(layout: &ShaderBuildLayout, path: &Path) -> Result<Self, String> {
        let metadata =
            std::fs::metadata(path).map_err(|err| format!("无法读取文件元数据 {}: {err}", path.display()))?;
        Ok(Self {
            path: layout.relative_slash_path(path),
            len: metadata.len(),
            modified_ms: Self::modified_ms(&metadata)?,
        })
    }

    fn modified_ms(metadata: &std::fs::Metadata) -> Result<u64, String> {
        let modified = metadata.modified().map_err(|err| format!("无法读取文件修改时间: {err}"))?;
        let millis = modified.duration_since(UNIX_EPOCH).unwrap_or_default().as_millis();
        Ok(millis.min(u128::from(u64::MAX)) as u64)
    }
}

fn main() -> Result<(), String> {
    TruvisLogger::init_with_file(LogFilePath::current_exe(TruvisPath::temp_dir()));

    let options = CliOptions::parse()?;
    ShaderBuildRunner::new(options.force)?.run()
}
