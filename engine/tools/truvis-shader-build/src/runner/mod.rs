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
    io::{ErrorKind, Read, Write},
    path::{Path, PathBuf},
};

use common::{ShaderCompileTask, ShaderCompiler, ShaderCompilerExecutables, ShaderCompilerType};
use dependency::{ShaderDependencyRoot, ShaderDependencyValidator, ShaderSourcePackage, ShaderSourceRoot, SourceLayer};
use glsl::GlslCompiler;
use hlsl::HlslCompiler;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use slang::SlangCompiler;
use truvis_path::TruvisPath;
use truvis_shader_manifest::{ShaderCompilerConfig, ShaderManifest, ShaderPackage};

const MANIFEST_VERSION: u32 = 9;
const COMPILER_ARGS_VERSION: &str = "shader-compiler-args-v5-validated-depfile";

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

    /// 配置输出根由调用者选择；根内不接受链接、junction 或其它 reparse point。
    /// 逐层检查现有祖先，也覆盖尚未创建的目标父目录；不跟随链接进行清理。
    fn check_output_path(&self, path: &Path, file: bool) -> Result<(), String> {
        let relative =
            path.strip_prefix(&self.output_dir).map_err(|_| format!("shader 产物不在输出根内: {}", path.display()))?;
        if file && relative.as_os_str().is_empty() {
            return Err("shader 输出根不能用作文件".to_string());
        }
        let mut current = self.output_dir.clone();
        for component in relative.components() {
            let std::path::Component::Normal(name) = component else {
                return Err(format!("shader 产物包含非 canonical 路径: {}", path.display()));
            };
            let name = name.to_str().ok_or_else(|| format!("shader 输出路径不是 UTF-8: {}", path.display()))?;
            #[cfg(windows)]
            if name.ends_with(['.', ' ']) || name.contains(':') {
                return Err(format!("shader 输出路径包含 Windows 别名或数据流: {}", path.display()));
            }
            current.push(name);
            let metadata = match std::fs::symlink_metadata(&current) {
                Ok(metadata) => metadata,
                Err(err) if err.kind() == ErrorKind::NotFound => continue,
                Err(err) => return Err(format!("无法检查 shader 输出路径 {}: {err}", current.display())),
            };
            let linked = metadata.file_type().is_symlink();
            #[cfg(windows)]
            let linked = {
                use std::os::windows::fs::MetadataExt;
                linked || metadata.file_attributes() & 0x400 != 0
            };
            if linked {
                return Err(format!("shader 输出根内不允许链接或 reparse point: {}", current.display()));
            }
            let expects_file = current == path && file;
            if (expects_file && !metadata.is_file()) || (!expects_file && !metadata.is_dir()) {
                return Err(format!("shader 输出文件/目录类型冲突: {}", current.display()));
            }
        }
        Ok(())
    }

    /// 与目标平台的文件身份保持一致；不让仅大小写变化触发旧产物删除。
    fn output_key(&self, path: &Path) -> Result<String, String> {
        self.relative_output_path(path)?;
        let root = self.output_dir.canonicalize().map_err(|err| format!("无法解析 shader 输出根: {err}"))?;
        let mut ancestor = path;
        let mut missing = Vec::new();
        while !ancestor.try_exists().map_err(|err| format!("无法检查 shader 路径 {}: {err}", ancestor.display()))?
        {
            missing.push(ancestor.file_name().ok_or_else(|| format!("shader 输出路径缺少文件名: {}", path.display()))?);
            ancestor = ancestor.parent().ok_or_else(|| format!("shader 输出路径缺少父目录: {}", path.display()))?;
        }
        // 已有祖先使用物理路径，避免 Windows 8.3 等别名绕过任务冲突检查。
        let mut resolved =
            ancestor.canonicalize().map_err(|err| format!("无法解析 shader 路径 {}: {err}", ancestor.display()))?;
        for name in missing.into_iter().rev() {
            resolved.push(name);
        }
        let key = resolved
            .strip_prefix(root)
            .map_err(|_| format!("shader 产物解析到输出根之外: {}", path.display()))?
            .to_string_lossy()
            .replace('\\', "/");
        #[cfg(windows)]
        let key = key.to_lowercase();
        Ok(key)
    }

    /// manifest 只拥有输出根内、非保留目录下的 SPIR-V 文件。
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
        let key = self.output_key(&path)?;
        if !key.ends_with(".spv") || key.starts_with(".deps/") || key.starts_with(".state/") {
            return Err(format!("非法 shader 受管输出路径: {relative_path}"));
        }
        self.check_output_path(&path, true)?;
        Ok(path)
    }
}

/// 完成全部预检后才允许消费的删除计划，同时保留本轮失败重试所需的输出归属。
struct ShaderOutputPlan {
    current_outputs: Vec<String>,
    owned_outputs: Vec<String>,
    stale_files: Vec<PathBuf>,
    directories: Vec<PathBuf>,
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
        let mut tasks = self.collect_tasks()?;
        let compiler_inputs = self.collect_compiler_inputs(&mut tasks)?;
        let global_config_changed = previous_manifest.is_none_or(|previous| {
            previous.compiler != *self.manifest.compiler() || previous.compiler_inputs != compiler_inputs
        });

        let previous_tasks = previous_manifest.map(ShaderBuildManifest::task_map).unwrap_or_default();
        let total_task_count = tasks.len();
        let mut next_task_manifests = Vec::with_capacity(total_task_count);
        let mut tasks_to_compile = Vec::new();

        for task in &tasks {
            let task_manifest = ShaderTaskManifest::from_task(&self.layout, task)?;
            let previous_task = previous_tasks.get(&task_manifest.task_id);
            let output_path = self.layout.resolve_output_path(&task_manifest.output_path);
            let needs_compile = self.force
                || global_config_changed
                || changed_packages.contains(&task.package_id)
                || previous_task.is_none_or(|old_task| old_task != &task_manifest)
                || !output_path.is_file()
                || !task.depfile_path.is_file()
                || self.dependency_validator.validate_compiler_dependencies(task).is_err();

            if needs_compile {
                tasks_to_compile.push(task.clone());
            }
            next_task_manifests.push(task_manifest);
        }

        let output_plan = self.preflight_outputs(&previous_state.managed_outputs, &tasks)?;
        let mut next_manifest = ShaderBuildManifest {
            version: MANIFEST_VERSION,
            completed: false,
            managed_outputs: output_plan.owned_outputs,
            compiler: self.manifest.compiler().clone(),
            compiler_inputs,
            packages: package_states,
            tasks: next_task_manifests,
        };
        if !tasks_to_compile.is_empty() || !output_plan.stale_files.is_empty() {
            // 保存新输入基准前使所有待编译任务失效；中断时未启动任务也不能复用旧 .d。
            for task in &tasks_to_compile {
                match std::fs::remove_file(&task.depfile_path) {
                    Ok(()) => {},
                    Err(err) if err.kind() == ErrorKind::NotFound => {},
                    Err(err) => return Err(format!("无法撤销 shader 完成标记 {}: {err}", task.depfile_path.display())),
                }
            }
            // 先记录旧/新归属；任何任务失败后，下一次都能清理已产生但不再需要的文件。
            next_manifest.save(&self.layout.state_manifest_path)?;
        }
        self.compile_tasks(&tasks_to_compile)?;
        for path in output_plan.stale_files {
            match std::fs::remove_file(&path) {
                Ok(()) => log::info!("Removed stale shader artifact: {}", path.display()),
                Err(err) if err.kind() == ErrorKind::NotFound => {}
                Err(err) => return Err(format!("无法删除旧 shader 产物 {}: {err}", path.display())),
            }
        }
        for path in output_plan.directories {
            match std::fs::remove_dir(&path) {
                Ok(()) => {}
                Err(err) if matches!(err.kind(), ErrorKind::DirectoryNotEmpty | ErrorKind::NotFound) => {}
                Err(err) => return Err(format!("无法清理空 shader 输出目录 {}: {err}", path.display())),
            }
        }
        next_manifest.completed = true;
        next_manifest.managed_outputs = output_plan.current_outputs;
        next_manifest.save(&self.layout.state_manifest_path)?;

        log::info!(
            "Shader compilation completed. compiled={}, skipped={}",
            tasks_to_compile.len(),
            total_task_count.saturating_sub(tasks_to_compile.len())
        );
        Ok(())
    }

    fn collect_tasks(&self) -> Result<Vec<ShaderCompileTask>, String> {
        let mut tasks = Vec::new();
        for package in &self.packages.packages {
            let entry_root = self.layout.resolve_manifest_path(&package.entry_root);
            let include_roots =
                package.include_roots.iter().map(|path| self.layout.resolve_manifest_path(path)).collect::<Vec<_>>();
            let allowed_dependency_roots = self.packages.allowed_dependency_paths(package, &self.layout);
            let output_prefix = Path::new(&package.output_prefix);

            for entry in walkdir::WalkDir::new(&entry_root) {
                let entry = entry.map_err(|err| format!("无法遍历 shader entry {}: {err}", entry_root.display()))?;
                if entry.path().is_file()
                    && let Some(task) = ShaderCompileTask::new(
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
                {
                    tasks.push(task);
                }
            }
        }

        tasks.sort_by(|left, right| {
            (&left.package_id, &left.entry_relative_path).cmp(&(&right.package_id, &right.entry_relative_path))
        });
        Ok(tasks)
    }

    /// 只解析实际任务需要的工具，并让指纹与 Command 使用同一个绝对路径。
    /// bare 名称明确从 PATH 查找，不依赖 Windows 隐式的当前目录搜索。
    fn resolve_compiler(executable: &Path) -> Result<PathBuf, String> {
        let resolved = if executable.components().count() == 1 && !executable.is_absolute() {
            let path = std::env::var_os("PATH").unwrap_or_default();
            std::env::split_paths(&path)
                .map(|dir| {
                    let candidate = dir.join(executable);
                    #[cfg(windows)]
                    if candidate.extension().is_none() {
                        return candidate.with_extension("exe");
                    }
                    candidate
                })
                .find(|candidate| candidate.is_file())
                .ok_or_else(|| format!("PATH 中找不到 shader 编译器: {}", executable.display()))?
        } else {
            executable.to_path_buf()
        };
        if !resolved.is_file() {
            return Err(format!("配置的 shader 编译器不存在: {}", resolved.display()));
        }
        std::fs::canonicalize(&resolved).map_err(|err| format!("无法解析 shader 编译器 {}: {err}", resolved.display()))
    }

    /// Windows SDK 常把实现放在 executable 旁的 DLL；只比较启动器会漏掉工具升级。
    /// DLL 只比较路径/长度/mtime，executable 保留内容 hash；目录外依赖或保留元数据替换需 --force。
    fn collect_compiler_inputs(&self, tasks: &mut [ShaderCompileTask]) -> Result<Vec<FileStamp>, String> {
        let mut resolved: BTreeMap<PathBuf, PathBuf> = BTreeMap::new();
        let mut files = BTreeSet::new();
        let mut directories = BTreeSet::new();
        for task in tasks {
            let executable = match resolved.entry(task.compiler_executable.clone()) {
                std::collections::btree_map::Entry::Occupied(entry) => entry.get().clone(),
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(Self::resolve_compiler(&task.compiler_executable)?).clone()
                }
            };
            files.insert(executable.clone());
            directories.insert(executable.parent().expect("absolute compiler path").to_path_buf());
            task.compiler_executable = executable;
        }
        for directory in directories {
            let entries = std::fs::read_dir(&directory)
                .map_err(|err| format!("无法读取 shader 工具目录 {}: {err}", directory.display()))?;
            for entry in entries {
                let entry = entry.map_err(|err| format!("无法遍历 shader 工具目录 {}: {err}", directory.display()))?;
                let path = entry.path();
                if path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("dll")) {
                    files.insert(path);
                }
            }
        }
        files.into_iter().map(|path| FileStamp::from_compiler_path(&self.layout, &path)).collect()
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
            for entry in walkdir::WalkDir::new(&root) {
                let entry = entry.map_err(|err| format!("无法遍历 shader 输入 {}: {err}", root.display()))?;
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

    /// 正式 depfile 同时是任务完成标记：编译器退出成功、输出存在且依赖合法后才能发布。
    /// 先移除旧标记，保证进程中止或失败清理受阻时，下一次构建仍会重试。
    /// 不保留旧 SPIR-V；构建失败后消费者不能继续使用本轮输出集合。
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
        let mut compiling = task.clone();
        compiling.depfile_path = task.depfile_path.with_extension("d.pending");
        for path in [&task.depfile_path, &compiling.depfile_path, &task.output_path] {
            match std::fs::remove_file(path) {
                Ok(()) => {}
                Err(err) if err.kind() == ErrorKind::NotFound => {}
                Err(err) => return Err(format!("无法删除旧 shader 编译产物 {}: {err}", path.display())),
            }
        }

        let result = (|| {
            Self::compiler_for(compiling.compiler_type).compile(&compiling)?;
            if !compiling.output_path.is_file() {
                return Err(format!("shader 编译器未生成输出: {}", compiling.output_path.display()));
            }
            self.dependency_validator.validate_compiler_dependencies(&compiling)?;
            std::fs::rename(&compiling.depfile_path, &task.depfile_path)
                .map_err(|err| format!("无法发布 shader depfile {}: {err}", task.depfile_path.display()))
        })();
        if let Err(mut err) = result {
            for path in [&task.output_path, &compiling.depfile_path] {
                match std::fs::remove_file(path) {
                    Ok(()) => {}
                    Err(remove_err) if remove_err.kind() == ErrorKind::NotFound => {}
                    Err(remove_err) => {
                        err.push_str(&format!("\n同时无法清理 shader 编译产物 {}: {remove_err}", path.display()));
                    }
                }
            }
            return Err(err);
        }
        Ok(())
    }

    /// 先收集并检查整个计划，再允许任何编译、删除或状态发布。
    /// 输出目录由单个构建器独占；不支持预检后由其它进程替换目录结构。
    fn preflight_outputs(
        &self,
        previous_outputs: &[String],
        tasks: &[ShaderCompileTask],
    ) -> Result<ShaderOutputPlan, String> {
        std::fs::create_dir_all(&self.layout.output_dir)
            .map_err(|err| format!("无法创建 shader 输出根 {}: {err}", self.layout.output_dir.display()))?;
        let mut destinations = BTreeMap::new();
        let mut current = BTreeMap::new();
        let mut current_depfiles = BTreeSet::new();
        let mut owned = BTreeMap::new();
        let mut stale_files = BTreeSet::new();
        let mut directories = Vec::new();
        let state_tmp = self.layout.state_manifest_path.with_extension("json.tmp");
        let mut files = vec![self.layout.state_manifest_path.clone(), state_tmp];
        for task in tasks {
            let relative = self.layout.relative_output_path(&task.output_path)?;
            self.layout.managed_output_from_manifest(&relative)?;
            current.insert(self.layout.output_key(&task.output_path)?, relative);
            current_depfiles.insert(self.layout.output_key(&task.depfile_path)?);
            files.extend([
                task.output_path.clone(),
                task.depfile_path.clone(),
                task.depfile_path.with_extension("d.pending"),
            ]);
        }
        for path in files {
            self.layout.check_output_path(&path, true)?;
            let key = self.layout.output_key(&path)?;
            if let Some(previous) = destinations.insert(key.clone(), path.clone()) {
                return Err(format!("shader 输出目的地重复: {} 与 {}", previous.display(), path.display()));
            }
            // 一个任务的文件不能同时成为另一个任务的父目录。
            for parent in Path::new(&key).ancestors().skip(1) {
                if destinations.contains_key(&parent.to_string_lossy().replace('\\', "/")) {
                    return Err(format!("shader 输出文件/目录前缀冲突: {}", path.display()));
                }
            }
            if destinations.keys().any(|other| other.starts_with(&format!("{key}/"))) {
                return Err(format!("shader 输出文件/目录前缀冲突: {}", path.display()));
            }
        }
        for relative in previous_outputs {
            let path = self.layout.managed_output_from_manifest(relative)?;
            let key = self.layout.output_key(&path)?;
            if !current.contains_key(&key) {
                if destinations
                    .keys()
                    .any(|other| other.starts_with(&format!("{key}/")) || key.starts_with(&format!("{other}/")))
                {
                    return Err(format!("旧 shader 文件与当前输出目录冲突: {}", path.display()));
                }
                stale_files.insert(path);
            }
            owned.insert(key, relative.clone());
        }
        owned.extend(current.clone());
        let dependency_prefix = format!("{}/", self.layout.output_key(&self.layout.dependency_dir)?);
        // 完整枚举也检查不在旧 manifest 中的目录链接，避免 WalkDir 跳过 junction 后掩盖边界问题。
        for entry in walkdir::WalkDir::new(&self.layout.output_dir).min_depth(1).contents_first(true) {
            let entry = entry.map_err(|err| format!("无法遍历 shader 输出目录: {err}"))?;
            let path = entry.path();
            self.layout.check_output_path(path, !entry.file_type().is_dir())?;
            if entry.file_type().is_dir() {
                directories.push(path.to_path_buf());
            } else {
                let key = self.layout.output_key(path)?;
                if key.starts_with(&dependency_prefix) && !current_depfiles.contains(&key) {
                    stale_files.insert(path.to_path_buf());
                }
            }
        }
        Ok(ShaderOutputPlan {
            current_outputs: current.into_values().collect(),
            owned_outputs: owned.into_values().collect(),
            stale_files: stale_files.into_iter().collect(),
            directories,
        })
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
    /// 完成状态描述整批构建；未完成输入也可比较，单任务成功由正式 depfile 与输出校验。
    completed: bool,
    managed_outputs: Vec<String>,
    compiler: ShaderCompilerConfig,
    compiler_inputs: Vec<FileStamp>,
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
        let temporary = path.with_extension("json.tmp");
        match std::fs::remove_file(&temporary) {
            Ok(()) => {}
            Err(err) if err.kind() == ErrorKind::NotFound => {}
            Err(err) => return Err(format!("无法移除 shader 临时状态 {}: {err}", temporary.display())),
        }
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|err| format!("无法创建 shader 临时状态 {}: {err}", temporary.display()))?;
        file.write_all(content).map_err(|err| format!("无法写入 shader 临时状态 {}: {err}", temporary.display()))?;
        drop(file);
        std::fs::rename(&temporary, path).map_err(|err| format!("无法发布 shader 状态 {}: {err}", path.display()))
    }
}

/// 只有当前版本的已完成状态可复用；失败记录仍保留输出归属。
/// v6/v7 只提取具有相同输出根语义的归属，不复用其输入缓存。
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
        if version == Some(u64::from(MANIFEST_VERSION)) {
            let manifest: ShaderBuildManifest = serde_json::from_value(value)
                .map_err(|err| format!("无法解析当前 shader manifest {}: {err}", path.display()))?;
            return Ok(Self {
                managed_outputs: manifest.managed_outputs.clone(),
                current: Some(manifest),
            });
        }
        let managed_outputs = if version == Some(8) {
            value.get("managed_outputs").and_then(serde_json::Value::as_array).into_iter().flatten()
                .filter_map(serde_json::Value::as_str).map(str::to_string).collect()
        } else if matches!(version, Some(6) | Some(7)) {
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
        Ok(Self {
            current: None,
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
    fingerprint: FileFingerprint,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
enum FileFingerprint {
    ContentHash(String),
    Metadata { length: u64, modified_ns: u128 },
}

impl FileStamp {
    fn from_compiler_path(layout: &ShaderBuildLayout, path: &Path) -> Result<Self, String> {
        if !path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("dll")) {
            return Self::from_path(layout, path);
        }
        let metadata = std::fs::metadata(path).map_err(|error| format!("无法检查 compiler DLL {}: {error}", path.display()))?;
        let modified_ns = metadata.modified().map_err(|error| error.to_string())?
            .duration_since(std::time::UNIX_EPOCH).map_err(|error| error.to_string())?.as_nanos();
        Ok(Self { path: layout.relative_slash_path(path), fingerprint: FileFingerprint::Metadata { length: metadata.len(), modified_ns } })
    }

    fn from_path(layout: &ShaderBuildLayout, path: &Path) -> Result<Self, String> {
        let mut file =
            std::fs::File::open(path).map_err(|err| format!("无法读取 shader 构建输入 {}: {err}", path.display()))?;
        let mut hash = Sha256::new();
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let read =
                file.read(&mut buffer).map_err(|err| format!("无法读取 shader 构建输入 {}: {err}", path.display()))?;
            if read == 0 {
                break;
            }
            hash.update(&buffer[..read]);
        }
        Ok(Self {
            path: layout.relative_slash_path(path),
            fingerprint: FileFingerprint::ContentHash(format!("{:x}", hash.finalize())),
        })
    }
}
