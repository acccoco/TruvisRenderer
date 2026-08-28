mod visual_studio;

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Component, Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use truvis_logs::{LogFilePath, TruvisLogger};
use truvis_path::TruvisPath;

const RUNTIME_PLAN_VERSION: u32 = 1;
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
}

impl CliOptions {
    fn parse() -> Result<Self, String> {
        let mut profile = BuildProfile::All;
        let mut force = false;
        let mut args = std::env::args().skip(1);

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--profile" => {
                    let value = args.next().ok_or_else(|| "--profile 需要参数：debug、release 或 all".to_string())?;
                    profile = if value.eq_ignore_ascii_case("all") {
                        BuildProfile::All
                    } else {
                        BuildProfile::One(BuildType::parse(&value)?)
                    };
                }
                "--force" | "-f" => force = true,
                "--help" | "-h" => {
                    return Err("Usage: cxx-build [--profile debug|release|all] [--force]".to_string());
                }
                _ => return Err(format!("Unsupported cxx-build arg '{arg}'")),
            }
        }

        Ok(Self { profile, force })
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

    fn cmake_output_dir(&self, toolchain: &str, build_type: BuildType) -> PathBuf {
        self.cxx_build_dir.join("output").join(toolchain).join(build_type.cmake_configuration())
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
        self.run_cmake(&["--preset", self.cmake_preset.configure], "configure")?;
        if let Err(err) = self.sync_compile_commands() {
            log::warn!("Skip compile_commands.json sync: {err}");
        }

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

        self.run_cmake(&args, &format!("build {}", build_type.label()))?;
        CxxRuntimePackager::deploy(&self.layout, self.cmake_preset.output_key, build_type)
    }

    fn run_cmake(&self, args: &[&str], action: &str) -> Result<(), String> {
        log::info!("Run cmake {}: cmake {}", action, args.join(" "));

        let status = std::process::Command::new("cmake")
            .current_dir(self.layout.cxx_project_dir())
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

    fn sync_compile_commands(&self) -> Result<(), String> {
        self.run_cmake(&["--preset", "clang-cl-debug"], "configure compile_commands")?;

        let source_path = self.layout.compile_commands_source();
        if !source_path.is_file() {
            return Err(format!("clang-cl-debug preset 没有生成 compile_commands.json: {}", source_path.display()));
        }

        let cxx_copy_path = self.layout.compile_commands_cxx_copy();
        let vscode_copy_path = self.layout.compile_commands_vscode_copy();
        CxxBuildFileHelper::copy_if_changed_to_path(&source_path, &cxx_copy_path)?;
        CxxBuildFileHelper::copy_if_changed_to_path(&source_path, &vscode_copy_path)?;

        log::info!("Synced compile_commands.json to {} and {}", cxx_copy_path.display(), vscode_copy_path.display());
        Ok(())
    }
}

struct CmakeOutputScanner<'a> {
    layout: &'a CxxBuildLayout,
    toolchain: &'a str,
    build_type: BuildType,
}

impl<'a> CmakeOutputScanner<'a> {
    fn new(layout: &'a CxxBuildLayout, toolchain: &'a str, build_type: BuildType) -> Self {
        Self {
            layout,
            toolchain,
            build_type,
        }
    }

    fn native_artifacts(&self) -> Result<Vec<PathBuf>, String> {
        let output_dir = self.layout.cmake_output_dir(self.toolchain, self.build_type);
        let mut source_paths = Vec::new();
        for entry in std::fs::read_dir(&output_dir)
            .map_err(|err| format!("无法读取 CMake 输出目录 {}: {err}", output_dir.display()))?
        {
            let entry = entry.map_err(|err| format!("无法读取 CMake 输出目录项 {}: {err}", output_dir.display()))?;
            let source_path = entry.path();
            if Self::is_native_artifact(&source_path) {
                source_paths.push(source_path);
            }
        }
        source_paths.sort();

        if source_paths.is_empty() {
            return Err(format!("CMake 输出目录中没有 native 产物: {}", output_dir.display()));
        }
        Ok(source_paths)
    }

    fn is_native_artifact(path: &Path) -> bool {
        path.extension().is_some_and(|suffix| {
            matches!(suffix.to_string_lossy().to_ascii_lowercase().as_str(), "dll" | "pdb" | "lib")
        })
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

        let native_outputs = CmakeOutputScanner::new(layout, toolchain, build_type);
        for source_path in native_outputs.native_artifacts()? {
            let destination = source_path
                .file_name()
                .ok_or_else(|| format!("无法获取 native 产物文件名: {}", source_path.display()))?
                .to_string_lossy()
                .to_string();
            deployment.deploy_file(&source_path, &destination)?;
        }

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
            deployment.deploy_file(&source_path, &artifact.destination)?;
        }

        deployment.remove_stale_previous_outputs(previous_manifest.as_ref())?;
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
    destinations: BTreeMap<String, PathBuf>,
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
        Ok(())
    }

    fn output_dirs(&self) -> [PathBuf; 2] {
        [self.cargo_output_dir.clone(), self.cargo_output_dir.join("examples")]
    }

    fn deploy_file(&mut self, source_path: &Path, destination: &str) -> Result<(), String> {
        let destination_path = Path::new(destination);
        let mut components = destination_path.components();
        let Some(Component::Normal(destination_name)) = components.next() else {
            return Err(format!("CXX runtime destination 必须是文件名: {destination}"));
        };
        if components.next().is_some() {
            return Err(format!("CXX runtime destination 不允许子目录: {destination}"));
        }

        let destination_key = destination_name.to_string_lossy().to_ascii_lowercase();
        if let Some(existing_source) = self.destinations.get(&destination_key) {
            if existing_source != source_path {
                return Err(format!(
                    "CXX runtime destination '{}' 同时来自 {} 和 {}",
                    destination,
                    existing_source.display(),
                    source_path.display()
                ));
            }
            return Ok(());
        }
        self.destinations.insert(destination_key, source_path.to_path_buf());

        for output_dir in self.output_dirs() {
            let deployed_path = output_dir.join(destination_name);
            let relative_path = CxxBuildFileHelper::relative_slash_path(self.workspace_dir, &deployed_path);
            self.managed_outputs.insert(relative_path);
            if CxxBuildFileHelper::copy_if_changed_to_path(source_path, &deployed_path)? {
                self.copied_files.push(CxxBuildFileHelper::relative_slash_path(self.workspace_dir, &deployed_path));
            }
        }
        Ok(())
    }

    fn remove_stale_previous_outputs(&self, previous_manifest: Option<&CxxDeploymentManifest>) -> Result<(), String> {
        let Some(previous_manifest) = previous_manifest else {
            return Ok(());
        };

        for previous_output in &previous_manifest.managed_outputs {
            if self.managed_outputs.contains(previous_output) {
                continue;
            }

            let output_path = self.workspace_dir.join(previous_output.replace('/', "\\"));
            if !output_path.starts_with(&self.cargo_output_dir) {
                return Err(format!("拒绝删除 Cargo 输出目录外的 CXX 托管文件: {}", output_path.display()));
            }
            CxxBuildFileHelper::remove_file_if_exists(&output_path)?;
        }
        Ok(())
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

        let source_modified = source_metadata
            .modified()
            .map_err(|err| format!("无法读取源文件修改时间 {}: {err}", source_path.display()))?;
        let destination_modified = destination_metadata
            .modified()
            .map_err(|err| format!("无法读取目标文件修改时间 {}: {err}", destination_path.display()))?;
        Ok(source_modified > destination_modified)
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
    TruvisLogger::init_with_file(LogFilePath::current_exe(TruvisPath::temp_dir()));

    let options = CliOptions::parse()?;
    let workspace_dir = TruvisPath::workspace_path();
    let target_dir = TruvisPath::target_path();
    let layout = CxxBuildLayout::new(workspace_dir, target_dir);
    if !layout.cxx_project_dir().join("CMakeLists.txt").is_file() {
        return Err(format!("CXX CMake project 不存在: {}", layout.cxx_project_dir().display()));
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
