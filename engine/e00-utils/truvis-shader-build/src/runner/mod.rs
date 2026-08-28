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
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use common::{ShaderCompileTask, ShaderCompiler, ShaderCompilerExecutables, ShaderCompilerType};
use dependency::{ShaderDependencyRoot, ShaderDependencyValidator, ShaderSourcePackage, ShaderSourceRoot, SourceLayer};
use glsl::GlslCompiler;
use hlsl::HlslCompiler;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use slang::SlangCompiler;
use truvis_path::TruvisPath;
use truvis_shader_manifest::{ShaderCompilerConfig, ShaderManifest, ShaderPackage};

const MANIFEST_VERSION: u32 = 6;
const COMPILER_ARGS_VERSION: &str = "shader-compiler-args-v4-depfile";

/// shader 编译流程的路径上下文。
struct ShaderBuildLayout {
    manifest_root: PathBuf,
    output_dir: PathBuf,
    dependency_dir: PathBuf,
    state_manifest_path: PathBuf,
    package_manifest_path: PathBuf,
}

impl ShaderBuildLayout {
    fn new(manifest: &ShaderManifest) -> Self {
        let output_dir = TruvisPath::shader_build_dir();
        Self {
            dependency_dir: output_dir.join(".deps"),
            state_manifest_path: output_dir.join(".state").join("shader-build.json"),
            package_manifest_path: manifest.manifest_path().to_path_buf(),
            manifest_root: manifest.root_dir().to_path_buf(),
            output_dir,
        }
    }

    fn resolve_manifest_path(&self, relative_path: &str) -> PathBuf {
        // 与迁移前保持一致，避免 Windows 路径分隔符进入 shader 调试信息并改变产物哈希。
        #[cfg(windows)]
        let relative_path = relative_path.replace('/', "\\");

        self.manifest_root.join(relative_path)
    }

    fn relative_slash_path(&self, path: &Path) -> String {
        path.strip_prefix(&self.manifest_root).unwrap_or(path).to_string_lossy().replace('\\', "/")
    }

    fn relative_output_path(&self, path: &Path) -> Result<String, String> {
        path.strip_prefix(&self.output_dir)
            .map(|relative| relative.to_string_lossy().replace('\\', "/"))
            .map_err(|_| format!("shader 构建产物不在配置的输出根内: {}", path.display()))
    }

    fn resolve_output_path(&self, relative_path: &str) -> PathBuf {
        #[cfg(windows)]
        let relative_path = relative_path.replace('/', "\\");

        self.output_dir.join(relative_path)
    }

    /// 旧 manifest 只能删除本工具输出目录内的文件。
    fn managed_output_from_manifest(&self, relative_path: &str) -> Result<PathBuf, String> {
        let relative = Path::new(relative_path);
        if relative_path.is_empty()
            || relative.components().any(|component| {
                matches!(
                    component,
                    std::path::Component::CurDir
                        | std::path::Component::ParentDir
                        | std::path::Component::RootDir
                        | std::path::Component::Prefix(_)
                )
            })
        {
            return Err(format!("拒绝旧 shader manifest 中的非 canonical 输出路径: {relative_path}"));
        }
        let path = self.resolve_output_path(relative_path);
        if !path.starts_with(&self.output_dir) {
            return Err(format!("拒绝删除 shader 输出目录外的 manifest 路径: {}", path.display()));
        }
        Ok(path)
    }
}

/// 已校验的 package 集合。
struct ShaderPackageSet {
    packages: Vec<ShaderPackage>,
}

impl ShaderPackageSet {
    fn new(mut packages: Vec<ShaderPackage>) -> Self {
        packages.sort_by(|left, right| left.id.cmp(&right.id));
        Self { packages }
    }

    fn require_directory(layout: &ShaderBuildLayout, relative_path: &str, package_id: &str) -> Result<(), String> {
        let path = layout.resolve_manifest_path(relative_path);
        if !path.is_dir() {
            return Err(format!("shader package '{}' 的目录不存在: {}", package_id, path.display()));
        }
        Ok(())
    }

    fn require_directories(&self, layout: &ShaderBuildLayout) -> Result<(), String> {
        for package in &self.packages {
            Self::require_directory(layout, &package.entry_root, &package.id)?;
            for path in package.include_roots.iter().chain(package.shared_inputs.iter().map(|input| &input.path)) {
                Self::require_directory(layout, path, &package.id)?;
            }
        }
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
    fn allowed_dependency_roots(
        &self,
        package: &ShaderPackage,
        layout: &ShaderBuildLayout,
    ) -> Vec<ShaderDependencyRoot> {
        let mut package_ids = BTreeSet::from([package.id.clone()]);
        self.collect_dependency_ids(&package.id, &mut package_ids);

        let mut roots = package_ids
            .iter()
            .flat_map(|package_id| {
                self.packages
                    .iter()
                    .find(|candidate| &candidate.id == package_id)
                    .expect("validated shader package disappeared")
                    .shared_inputs
                    .iter()
            })
            .map(|input| ShaderDependencyRoot {
                path: layout.resolve_manifest_path(&input.path),
                layer: input.layer.into(),
            })
            .collect::<Vec<_>>();
        roots.sort_by(|left, right| left.path.cmp(&right.path));
        roots.dedup_by(|left, right| left.path == right.path);
        roots
    }

    fn allowed_dependency_paths(&self, package: &ShaderPackage, layout: &ShaderBuildLayout) -> Vec<PathBuf> {
        self.allowed_dependency_roots(package, layout).into_iter().map(|root| root.path).collect()
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
pub struct ShaderBuildRunner {
    manifest: ShaderManifest,
    layout: ShaderBuildLayout,
    packages: ShaderPackageSet,
    compiler_executables: ShaderCompilerExecutables,
    dependency_validator: ShaderDependencyValidator,
    force: bool,
}

impl ShaderBuildRunner {
    pub fn new(manifest: ShaderManifest, force: bool) -> Result<Self, String> {
        let layout = ShaderBuildLayout::new(&manifest);
        let packages = ShaderPackageSet::new(manifest.packages().to_vec());
        packages.require_directories(&layout)?;
        let compiler_executables = ShaderCompilerExecutables::from_manifest(&manifest);
        Self::require_configured_compiler(&manifest.compiler().slangc, &manifest.slangc_executable())?;
        Self::require_configured_compiler(&manifest.compiler().glslc, &manifest.glslc_executable())?;
        Self::require_configured_compiler(&manifest.compiler().dxc, &manifest.dxc_executable())?;
        let dependency_validator = ShaderDependencyValidator::new(&layout.manifest_root)?;
        Ok(Self {
            manifest,
            layout,
            packages,
            compiler_executables,
            dependency_validator,
            force,
        })
    }

    fn require_configured_compiler(configured: &str, resolved: &Path) -> Result<(), String> {
        let configured_path = Path::new(configured);
        let resolves_to_file = configured_path.is_absolute() || configured.contains('/') || configured.contains('\\');
        if resolves_to_file && !resolved.is_file() {
            return Err(format!("配置的 shader 编译器不存在: {}", resolved.display()));
        }
        Ok(())
    }

    pub fn run(&self) -> Result<(), String> {
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
        let global_config_changed =
            previous_manifest.is_none_or(|previous| previous.compiler != *self.manifest.compiler());

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
            let output_path = self.layout.resolve_output_path(&task_manifest.output_path);
            let needs_compile = self.force
                || global_config_changed
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
            compiler: self.manifest.compiler().clone(),
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
            let entry_root = self.layout.resolve_manifest_path(&package.entry_root);
            let include_roots =
                package.include_roots.iter().map(|path| self.layout.resolve_manifest_path(path)).collect::<Vec<_>>();
            let allowed_dependency_roots = self.packages.allowed_dependency_paths(package, &self.layout);
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
                            &self.compiler_executables,
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
                    .shared_inputs
                    .iter()
                    .map(|input| ShaderSourceRoot {
                        path: self.layout.resolve_manifest_path(&input.path),
                        layer: input.layer.into(),
                    })
                    .collect::<Vec<_>>();
                source_roots.push(ShaderSourceRoot {
                    path: self.layout.resolve_manifest_path(&package.entry_root),
                    layer: SourceLayer::Entry,
                });

                ShaderSourcePackage {
                    package_id: package.id.clone(),
                    include_roots: package
                        .include_roots
                        .iter()
                        .map(|path| self.layout.resolve_manifest_path(path))
                        .collect(),
                    source_roots,
                    allowed_dependency_roots: self.packages.allowed_dependency_roots(package, &self.layout),
                }
            })
            .collect::<Vec<_>>();

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
    fn collect_shared_inputs(&self, package: &ShaderPackage) -> Result<Vec<FileStamp>, String> {
        let entry_root = self.layout.resolve_manifest_path(&package.entry_root);
        let include_roots =
            package.include_roots.iter().map(|path| self.layout.resolve_manifest_path(path)).collect::<Vec<_>>();
        let allowed_dependency_roots = self.packages.allowed_dependency_paths(package, &self.layout);
        let output_prefix = Path::new(&package.output_prefix);
        let mut inputs = BTreeMap::new();

        for root in package
            .shared_inputs
            .iter()
            .map(|input| self.layout.resolve_manifest_path(&input.path))
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
                        &self.compiler_executables,
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
            .map(|task| self.layout.relative_output_path(&task.output_path))
            .collect::<Result<BTreeSet<_>, _>>()?;

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
    compiler: ShaderCompilerConfig,
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

/// 当前版本 state 中的输出路径以 `shader_build` 为根；旧版本使用 manifest 根，首次运行只触发全量重编，
/// 不能按新语义复用其增量状态或输出集合。
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
        let version = value.get("version").and_then(serde_json::Value::as_u64);
        let managed_outputs = if version == Some(u64::from(MANIFEST_VERSION)) {
            value
                .get("tasks")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|task| task.get("output_path").and_then(serde_json::Value::as_str))
                .map(str::to_string)
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
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
    config: ShaderPackage,
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
    compiler_executable: String,
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
            output_path: layout.relative_output_path(&task.output_path)?,
            dependency_path: layout.relative_output_path(&task.depfile_path)?,
            shader_input: FileStamp::from_path(layout, &task.shader_path)?,
            shader_stage: format!("{:?}", task.shader_stage),
            compiler_type: format!("{:?}", task.compiler_type),
            compiler_executable: layout.relative_slash_path(&task.compiler_executable),
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
