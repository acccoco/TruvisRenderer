mod visual_studio;

use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    path::{Component, Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use truvis_logs::{LogFilePath, TruvisLogger};
use truvis_path::TruvisPath;

const RUNTIME_PLAN_VERSION: u32 = 2;
const DEPLOYMENT_MANIFEST_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BuildType {
    Debug,
    Release,
}

impl BuildType {
    fn parse(value: &str) -> Result<Self, String> {
        match value.to_ascii_lowercase().as_str() {
            "debug" => Ok(Self::Debug),
            "release" => Ok(Self::Release),
            _ => Err(format!("Unsupported CXX profile '{value}'. Use debug, release, or all.")),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Release => "release",
        }
    }

    fn cmake_configuration(self) -> &'static str {
        match self {
            Self::Debug => "Debug",
            Self::Release => "Release",
        }
    }

    fn cargo_output_dir(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Release => "release",
        }
    }
}

#[derive(Clone, Copy)]
enum BuildProfile {
    One(BuildType),
    All,
}

impl BuildProfile {
    fn build_types(self) -> &'static [BuildType] {
        match self {
            Self::One(BuildType::Debug) => &[BuildType::Debug],
            Self::One(BuildType::Release) => &[BuildType::Release],
            Self::All => &[BuildType::Debug, BuildType::Release],
        }
    }
}

struct CliOptions {
    profile: BuildProfile,
    force: bool,
    compile_commands: bool,
    verbose: bool,
}

impl CliOptions {
    fn parse() -> Result<Self, String> {
        let mut profile = BuildProfile::All;
        let mut force = false;
        let mut compile_commands = false;
        let mut verbose = false;
        let mut profile_specified = false;
        let mut args = std::env::args().skip(1);

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--profile" => {
                    profile_specified = true;
                    let value = args.next().ok_or_else(|| "--profile 需要参数：debug、release 或 all".to_string())?;
                    profile = if value.eq_ignore_ascii_case("all") {
                        BuildProfile::All
                    } else {
                        BuildProfile::One(BuildType::parse(&value)?)
                    };
                }
                "--force" | "-f" => force = true,
                "--compile-commands" => compile_commands = true,
                "--verbose" => verbose = true,
                "--help" | "-h" => {
                    return Err(
                        "Usage: cxx-build [--profile debug|release|all] [--force] [--compile-commands] [--verbose]"
                            .to_string(),
                    );
                }
                _ => return Err(format!("Unsupported cxx-build arg '{arg}'")),
            }
        }

        if compile_commands && (force || profile_specified) {
            return Err("--compile-commands 不能与 --profile 或 --force 一起使用".to_string());
        }

        Ok(Self {
            profile,
            force,
            compile_commands,
            verbose,
        })
    }
}

#[derive(Clone)]
struct CxxBuildLayout {
    workspace_dir: PathBuf,
    cxx_project_dir: PathBuf,
    cxx_build_dir: PathBuf,
    cargo_target_dir: PathBuf,
}

impl CxxBuildLayout {
    fn new(workspace_dir: PathBuf, cargo_target_dir: PathBuf) -> Self {
        Self {
            cxx_project_dir: workspace_dir.join("cxx"),
            cxx_build_dir: cargo_target_dir.join("cxx"),
            workspace_dir,
            cargo_target_dir,
        }
    }

    fn cxx_project_dir(&self) -> &Path {
        &self.cxx_project_dir
    }

    fn cxx_build_dir(&self) -> &Path {
        &self.cxx_build_dir
    }

    fn deployment_manifest_path(&self, build_type: BuildType) -> PathBuf {
        self.cxx_build_dir.join(".state").join(format!("deployment-{}.json", build_type.label()))
    }

    fn cmake_binary_dir(&self, toolchain: &str) -> PathBuf {
        self.cxx_build_dir.join("cmake").join(toolchain)
    }

    fn runtime_plan_path(&self, toolchain: &str, build_type: BuildType) -> PathBuf {
        self.cmake_binary_dir(toolchain)
            .join("runtime")
            .join(format!("truvis-runtime-{}.json", build_type.cmake_configuration()))
    }

    fn cargo_output_dir(&self, build_type: BuildType) -> PathBuf {
        self.cargo_target_dir.join(build_type.cargo_output_dir())
    }

    fn compile_commands_source(&self) -> PathBuf {
        self.cxx_build_dir.join("cmake/clang-cl/Debug/compile_commands.json")
    }

    fn compile_commands_cxx_copy(&self) -> PathBuf {
        self.cxx_build_dir.join("compile_commands.json")
    }

    fn compile_commands_vscode_copy(&self) -> PathBuf {
        self.workspace_dir.join(".vscode/compile_commands.json")
    }
}

struct CxxBuildRunner {
    layout: CxxBuildLayout,
    cmake_preset: visual_studio::CmakePreset,
    force: bool,
}

impl CxxBuildRunner {
    fn new(layout: CxxBuildLayout, cmake_preset: visual_studio::CmakePreset, force: bool) -> Self {
        Self {
            layout,
            cmake_preset,
            force,
        }
    }

    fn run(&self, profile: BuildProfile) -> Result<(), String> {
        // Configure 刷新 CMake build graph；compiler/linker 是否执行完全由 generator 决定，Rust 不维护 native 输入快照。
        Self::run_cmake(&self.layout, &["--preset", self.cmake_preset.configure], "configure")?;

        for build_type in profile.build_types() {
            self.run_profile(*build_type)?;
        }
        Ok(())
    }

    fn run_profile(&self, build_type: BuildType) -> Result<(), String> {
        let build_preset = match build_type {
            BuildType::Debug => self.cmake_preset.build_debug,
            BuildType::Release => self.cmake_preset.build_release,
        };
        let mut args = vec!["--build", "--preset", build_preset];
        if self.force {
            // `--force` 只向 CMake 请求 clean rebuild，不直接删除 object、cache 或输出目录。
            args.push("--clean-first");
        }

        Self::run_cmake(&self.layout, &args, &format!("build {}", build_type.label()))?;
        CxxRuntimePackager::deploy(&self.layout, self.cmake_preset.output_key, build_type)
    }

    fn run_cmake(layout: &CxxBuildLayout, args: &[&str], action: &str) -> Result<(), String> {
        log::info!("Run cmake {}: cmake {}", action, args.join(" "));

        let status = std::process::Command::new("cmake")
            .current_dir(layout.cxx_project_dir())
            .args(args)
            .status()
            .map_err(|err| format!("无法执行 cmake {action}: {err}"))?;

        if status.success() {
            return Ok(());
        }

        Err(format!(
            "cmake {action} 失败，退出码: {}",
            status.code().map_or_else(|| "unknown".to_string(), |code| code.to_string())
        ))
    }

    fn sync_compile_commands(layout: &CxxBuildLayout) -> Result<(), String> {
        Self::run_cmake(layout, &["--preset", "clang-cl-debug"], "configure compile_commands")?;

        let source_path = layout.compile_commands_source();
        if !source_path.is_file() {
            return Err(format!("clang-cl-debug preset 没有生成 compile_commands.json: {}", source_path.display()));
        }

        let cxx_copy_path = layout.compile_commands_cxx_copy();
        let vscode_copy_path = layout.compile_commands_vscode_copy();
        CxxBuildFileHelper::copy_if_changed_to_path(&source_path, &cxx_copy_path)?;
        CxxBuildFileHelper::copy_if_changed_to_path(&source_path, &vscode_copy_path)?;

        log::info!("Synced compile_commands.json to {} and {}", cxx_copy_path.display(), vscode_copy_path.display());
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct CxxRuntimePlan {
    version: u32,
    configuration: String,
    artifacts: Vec<CxxRuntimeArtifact>,
}

impl CxxRuntimePlan {
    fn load(path: &Path, build_type: BuildType) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|err| format!("无法读取 CXX runtime plan {}: {err}", path.display()))?;
        let plan: Self = serde_json::from_str(&content)
            .map_err(|err| format!("无法解析 CXX runtime plan {}: {err}", path.display()))?;

        if plan.version != RUNTIME_PLAN_VERSION {
            return Err(format!(
                "不支持的 CXX runtime plan 版本 {}，期望 {}: {}",
                plan.version,
                RUNTIME_PLAN_VERSION,
                path.display()
            ));
        }
        if plan.configuration != build_type.cmake_configuration() {
            return Err(format!(
                "CXX runtime plan configuration 为 {}，期望 {}: {}",
                plan.configuration,
                build_type.cmake_configuration(),
                path.display()
            ));
        }
        if plan.artifacts.is_empty() {
            return Err(format!("CXX runtime plan 不允许为空: {}", path.display()));
        }
        Ok(plan)
    }
}

#[derive(Debug, Deserialize)]
struct CxxRuntimeArtifact {
    target: String,
    source: String,
    destination: String,
    required: bool,
}

struct CxxRuntimePackager;

impl CxxRuntimePackager {
    fn deploy(layout: &CxxBuildLayout, toolchain: &str, build_type: BuildType) -> Result<(), String> {
        let manifest_path = layout.deployment_manifest_path(build_type);
        let previous_manifest = CxxDeploymentManifest::load(&manifest_path)?;
        let plan_path = layout.runtime_plan_path(toolchain, build_type);
        let runtime_plan = CxxRuntimePlan::load(&plan_path, build_type)?;
        let mut deployment = CargoRuntimeDeployment::new(layout, build_type);
        deployment.ensure_output_dirs()?;

        for artifact in runtime_plan.artifacts {
            let source_path = PathBuf::from(&artifact.source);
            if !source_path.is_file() {
                if artifact.required {
                    return Err(format!(
                        "CXX target '{}' 缺少 required runtime 文件: {}",
                        artifact.target,
                        source_path.display()
                    ));
                }
                log::warn!("Skip optional CXX runtime for target '{}': {}", artifact.target, source_path.display());
                continue;
            }
            deployment.plan_file(&source_path, &artifact.destination)?;
        }

        let stale_paths = deployment.stale_previous_outputs(previous_manifest.as_ref())?;
        // 在第一次复制前保存所有权；失败重试或切换分支后仍能清理本轮写入的文件。
        let mut pending_manifest = CxxDeploymentManifest::from_deployment(&deployment);
        if let Some(previous) = previous_manifest {
            pending_manifest.managed_outputs.extend(previous.managed_outputs);
            pending_manifest.managed_outputs.sort();
            pending_manifest.managed_outputs.dedup();
        }
        pending_manifest.save(&manifest_path)?;
        deployment.copy_files()?;
        for path in stale_paths {
            CxxBuildFileHelper::remove_file_if_exists(&path)?;
        }
        CxxDeploymentManifest::from_deployment(&deployment).save(&manifest_path)?;

        if deployment.copied_files().is_empty() {
            log::info!("CXX {} Cargo deployment already up to date.", build_type.label());
        } else {
            log::info!("Copied CXX {} files: {:#?}", build_type.label(), deployment.copied_files());
        }
        Ok(())
    }
}

struct CargoRuntimeDeployment<'a> {
    workspace_dir: &'a Path,
    cargo_output_dir: PathBuf,
    managed_outputs: BTreeSet<String>,
    destinations: BTreeMap<String, (PathBuf, String)>,
    copied_files: Vec<String>,
}

impl<'a> CargoRuntimeDeployment<'a> {
    fn new(layout: &'a CxxBuildLayout, build_type: BuildType) -> Self {
        Self {
            workspace_dir: &layout.workspace_dir,
            cargo_output_dir: layout.cargo_output_dir(build_type),
            managed_outputs: BTreeSet::new(),
            destinations: BTreeMap::new(),
            copied_files: Vec::new(),
        }
    }

    fn ensure_output_dirs(&self) -> Result<(), String> {
        for output_dir in self.output_dirs() {
            std::fs::create_dir_all(&output_dir)
                .map_err(|err| format!("无法创建 Cargo 输出目录 {}: {err}", output_dir.display()))?;
        }
        let canonical_root =
            self.cargo_output_dir.canonicalize().map_err(|err| format!("无法解析 Cargo 输出目录: {err}"))?;
        for output_dir in self.output_dirs() {
            let canonical_dir = output_dir
                .canonicalize()
                .map_err(|err| format!("无法解析 Cargo 部署目录 {}: {err}", output_dir.display()))?;
            if !canonical_dir.starts_with(&canonical_root) {
                return Err(format!("Cargo 部署目录链接指向 profile 外: {}", output_dir.display()));
            }
        }
        Ok(())
    }

    fn output_dirs(&self) -> [PathBuf; 2] {
        [self.cargo_output_dir.clone(), self.cargo_output_dir.join("examples")]
    }

    /// 完整计划只登记来源和目标，不覆盖文件；任一前置条件失败都在复制前返回。
    fn plan_file(&mut self, source_path: &Path, destination: &str) -> Result<(), String> {
        let destination_path = Path::new(destination);
        let mut components = destination_path.components();
        let Some(Component::Normal(destination_name)) = components.next() else {
            return Err(format!("CXX runtime destination 必须是文件名: {destination}"));
        };
        if components.next().is_some() {
            return Err(format!("CXX runtime destination 不允许子目录: {destination}"));
        }
        let source_path = source_path
            .canonicalize()
            .map_err(|err| format!("无法解析 CXX 源文件 {}: {err}", source_path.display()))?;

        let destination_key = destination_name.to_string_lossy().to_ascii_lowercase();
        if let Some((existing_source, _)) = self.destinations.get(&destination_key) {
            if existing_source != &source_path {
                return Err(format!(
                    "CXX runtime destination '{}' 同时来自 {} 和 {}",
                    destination,
                    existing_source.display(),
                    source_path.display()
                ));
            }
            return Ok(());
        }
        self.destinations.insert(destination_key, (source_path, destination.to_string()));

        for output_dir in self.output_dirs() {
            let deployed_path = output_dir.join(destination_name);
            if deployed_path.is_dir() {
                return Err(format!("CXX 目标文件被目录占用: {}", deployed_path.display()));
            }
            let relative_path = CxxBuildFileHelper::relative_slash_path(self.workspace_dir, &deployed_path);
            self.managed_outputs.insert(relative_path);
        }
        Ok(())
    }

    fn copy_files(&mut self) -> Result<(), String> {
        for (source_path, destination) in self.destinations.values() {
            for output_dir in self.output_dirs() {
                let deployed_path = output_dir.join(destination);
                if CxxBuildFileHelper::copy_if_changed_to_path(source_path, &deployed_path)? {
                    self.copied_files.push(CxxBuildFileHelper::relative_slash_path(self.workspace_dir, &deployed_path));
                }
            }
        }
        Ok(())
    }

    /// 先校验整份清单，损坏记录不能让前面的合法记录被部分删除。
    /// 父目录解析同时阻止 examples junction 将删除重定向到 profile 外。
    fn stale_previous_outputs(
        &self,
        previous_manifest: Option<&CxxDeploymentManifest>,
    ) -> Result<Vec<PathBuf>, String> {
        let Some(previous_manifest) = previous_manifest else {
            return Ok(Vec::new());
        };

        let canonical_output_dir = self
            .cargo_output_dir
            .canonicalize()
            .map_err(|err| format!("无法解析 Cargo 输出目录 {}: {err}", self.cargo_output_dir.display()))?;
        let mut stale_paths = Vec::new();
        for previous_output in &previous_manifest.managed_outputs {
            let relative_path = PathBuf::from(previous_output);
            if !relative_path.components().all(|component| matches!(component, Component::Normal(_))) {
                return Err(format!("CXX 托管路径必须是无父目录跳转的相对路径: {previous_output}"));
            }
            let output_path = self.workspace_dir.join(relative_path);
            let parent = output_path
                .parent()
                .filter(|parent| self.output_dirs().iter().any(|allowed| allowed == parent))
                .ok_or_else(|| format!("拒绝删除 Cargo 部署位置外的 CXX 托管文件: {}", output_path.display()))?;
            let canonical_parent = parent
                .canonicalize()
                .map_err(|err| format!("无法解析 CXX 托管文件父目录 {}: {err}", parent.display()))?;
            if !canonical_parent.starts_with(&canonical_output_dir) {
                return Err(format!("拒绝通过目录链接删除 Cargo 输出目录外的 CXX 托管文件: {}", output_path.display()));
            }
            // Windows 文件名不区分大小写；仅改目标大小写不能把刚部署的文件当作旧项删除。
            let destination_key = output_path.file_name().unwrap().to_string_lossy().to_ascii_lowercase();
            if self.destinations.contains_key(&destination_key) {
                continue;
            }
            stale_paths.push(output_path);
        }

        Ok(stale_paths)
    }

    fn managed_outputs(&self) -> Vec<String> {
        self.managed_outputs.iter().cloned().collect()
    }

    fn copied_files(&self) -> &[String] {
        &self.copied_files
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct CxxDeploymentManifest {
    version: u32,
    managed_outputs: Vec<String>,
}

impl CxxDeploymentManifest {
    fn from_deployment(deployment: &CargoRuntimeDeployment<'_>) -> Self {
        Self {
            version: DEPLOYMENT_MANIFEST_VERSION,
            managed_outputs: deployment.managed_outputs(),
        }
    }

    fn load(path: &Path) -> Result<Option<Self>, String> {
        if !path.is_file() {
            return Ok(None);
        }

        let content = std::fs::read_to_string(path)
            .map_err(|err| format!("无法读取 CXX deployment manifest {}: {err}", path.display()))?;
        let manifest: Self = serde_json::from_str(&content)
            .map_err(|err| format!("无法解析 CXX deployment manifest {}: {err}", path.display()))?;
        if manifest.version != DEPLOYMENT_MANIFEST_VERSION {
            return Err(format!(
                "不支持的 CXX deployment manifest 版本 {}，期望 {}: {}",
                manifest.version,
                DEPLOYMENT_MANIFEST_VERSION,
                path.display()
            ));
        }
        Ok(Some(manifest))
    }

    fn save(&self, path: &Path) -> Result<(), String> {
        let content = serde_json::to_string_pretty(self)
            .map_err(|err| format!("无法序列化 CXX deployment manifest {}: {err}", path.display()))?;
        CxxBuildFileHelper::write_if_changed(path, content.as_bytes())
    }
}

struct CxxBuildFileHelper;

impl CxxBuildFileHelper {
    fn copy_if_changed_to_path(source_path: &Path, destination_path: &Path) -> Result<bool, String> {
        let parent =
            destination_path.parent().ok_or_else(|| format!("无法获取目标目录: {}", destination_path.display()))?;
        std::fs::create_dir_all(parent).map_err(|err| format!("无法创建目录 {}: {err}", parent.display()))?;

        if !Self::needs_copy(source_path, destination_path)? {
            return Ok(false);
        }

        std::fs::copy(source_path, destination_path)
            .map_err(|err| format!("无法复制 {} 到 {}: {err}", source_path.display(), destination_path.display()))?;
        Ok(true)
    }

    /// 切换分支、工具链或恢复缓存后，源文件可能同大小且时间更早。
    /// 用有界缓冲核对内容，时间戳不能作为 DLL 或 import library 的内容身份。
    fn needs_copy(source_path: &Path, destination_path: &Path) -> Result<bool, String> {
        if !destination_path.is_file() {
            return Ok(true);
        }

        let source_metadata =
            std::fs::metadata(source_path).map_err(|err| format!("无法读取源文件 {}: {err}", source_path.display()))?;
        let destination_metadata = std::fs::metadata(destination_path)
            .map_err(|err| format!("无法读取目标文件 {}: {err}", destination_path.display()))?;
        if source_metadata.len() != destination_metadata.len() {
            return Ok(true);
        }

        let mut source = std::fs::File::open(source_path)
            .map_err(|err| format!("无法打开源文件 {}: {err}", source_path.display()))?;
        let mut destination = std::fs::File::open(destination_path)
            .map_err(|err| format!("无法打开目标文件 {}: {err}", destination_path.display()))?;
        let mut source_buffer = [0u8; 64 * 1024];
        let mut destination_buffer = [0u8; 64 * 1024];
        let mut remaining = source_metadata.len();
        while remaining > 0 {
            let count = remaining.min(source_buffer.len() as u64) as usize;
            source
                .read_exact(&mut source_buffer[..count])
                .map_err(|err| format!("无法读取源文件 {}: {err}", source_path.display()))?;
            destination
                .read_exact(&mut destination_buffer[..count])
                .map_err(|err| format!("无法读取目标文件 {}: {err}", destination_path.display()))?;
            if source_buffer[..count] != destination_buffer[..count] {
                return Ok(true);
            }
            remaining -= count as u64;
        }
        Ok(false)
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
        std::fs::write(&temporary, content).map_err(|err| format!("无法写入文件 {}: {err}", temporary.display()))?;
        std::fs::rename(&temporary, path).map_err(|err| format!("无法发布文件 {}: {err}", path.display()))
    }

    fn remove_file_if_exists(path: &Path) -> Result<(), String> {
        if !path.exists() {
            return Ok(());
        }
        std::fs::remove_file(path).map_err(|err| format!("无法删除旧 CXX 托管文件 {}: {err}", path.display()))
    }

    fn relative_slash_path(root: &Path, path: &Path) -> String {
        path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/")
    }
}

fn main() -> Result<(), String> {
    let options = CliOptions::parse()?;
    TruvisLogger::init_tool_with_file(LogFilePath::current_exe(TruvisPath::temp_dir()), options.verbose);
    let workspace_dir = TruvisPath::workspace_path();
    let target_dir = TruvisPath::target_path();
    let layout = CxxBuildLayout::new(workspace_dir, target_dir);
    if !layout.cxx_project_dir().join("CMakeLists.txt").is_file() {
        return Err(format!("CXX CMake project 不存在: {}", layout.cxx_project_dir().display()));
    }

    if options.compile_commands {
        return CxxBuildRunner::sync_compile_commands(&layout);
    }

    log::info!("cxx_project_dir: {:?}", layout.cxx_project_dir());
    log::info!("cxx_build_dir: {:?}", layout.cxx_build_dir());

    let cmake_preset = visual_studio::select_cmake_preset()?;
    log::info!(
        "Using {} CMake presets: {}, {}, {}",
        cmake_preset.visual_studio_name,
        cmake_preset.configure,
        cmake_preset.build_debug,
        cmake_preset.build_release
    );

    CxxBuildRunner::new(layout, cmake_preset, options.force).run(options.profile)
}
