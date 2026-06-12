mod visual_studio;

use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use serde::{Deserialize, Serialize};
use truvis_logs::{LogFilePath, TruvisLogger};
use truvis_path::TruvisPath;

const MANIFEST_VERSION: u32 = 1;

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

    fn label(&self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Release => "release",
        }
    }

    fn cmake_output_dir(&self) -> &'static str {
        match self {
            Self::Debug => "Debug",
            Self::Release => "Release",
        }
    }

    fn cargo_output_dir(&self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Release => "release",
        }
    }

    fn streamline_runtime_dir(&self, streamline_sdk_root: &Path) -> PathBuf {
        // Streamline SDK 同时提供 production 和 development runtime：
        // - Debug 使用 development，便于 SL 输出更完整的诊断信息。
        // - Release 使用 production，避免把开发向 DLL 带入发布运行目录。
        match self {
            Self::Debug => streamline_sdk_root.join("bin").join("x64").join("development"),
            Self::Release => streamline_sdk_root.join("bin").join("x64"),
        }
    }
}

#[derive(Clone, Copy)]
enum BuildProfile {
    One(BuildType),
    All,
}

impl BuildProfile {
    // BuildProfile 只表达 CLI 层选择的构建范围，不携带任何路径或构建状态。
    // 后续真正的 Debug/Release 差异由 BuildType 和 CxxBuildLayout 共同决定，
    // 这样 profile 解析不会反向知道 CMake/Cargo 的目录细节。
    fn build_types(&self) -> &'static [BuildType] {
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
                "--force" | "-f" => {
                    force = true;
                }
                "--help" | "-h" => {
                    return Err("Usage: cxx-build [--profile debug|release|all] [--force]".to_string());
                }
                _ => return Err(format!("Unsupported cxx-build arg '{arg}'")),
            }
        }

        Ok(Self { profile, force })
    }
}

// 默认复制 DLSS Super Resolution 与 Ray Reconstruction 所需 DLL。
// NvLowLatencyVk.dll 是 SL Vulkan backend 启动时加载的低延迟 helper，不表示启用 Reflex feature。
// Frame Generation、Reflex、NIS、DirectSR 等 feature 的 DLL 不在这里出现；
// 后续启用新 feature 时，应先扩展 Rust 侧 feature flags，再同步扩展这个清单。
const STREAMLINE_REQUIRED_DLLS: &[&str] = &[
    "sl.interposer.dll",
    "sl.common.dll",
    "sl.pcl.dll",
    "sl.dlss.dll",
    "nvngx_dlss.dll",
    "sl.dlss_d.dll",
    "nvngx_dlssd.dll",
    "NvLowLatencyVk.dll",
];

// Debug 使用 development runtime，并保留可由 TRUVIS_STREAMLINE_IMGUI 启用的 Streamline ImGui 调试 UI。
// sl.imgui 只进入 Debug 运行目录，Release 继续保持 DLSS SR 最小 runtime。
const STREAMLINE_DEBUG_OPTIONAL_DLLS: &[&str] = &["sl.imgui.dll"];

// 旧版本曾把 WinPixEventRuntime.dll 当作 Debug optional runtime 复制。
// 当前 Vulkan 路径不依赖 PIX runtime，保留清理项是为了删除已有构建目录中的旧残留。
const STREAMLINE_REMOVED_MANAGED_DLLS: &[&str] = &["WinPixEventRuntime.dll"];

#[derive(Clone)]
struct CxxBuildLayout {
    workspace_dir: PathBuf,
    cxx_build_dir: PathBuf,
    cargo_target_dir: PathBuf,
}

impl CxxBuildLayout {
    // 所有派生路径都从 workspace 和 Cargo target 根目录计算，避免调用方散落拼接
    // build/cxx、build/{profile}、.vscode 等约定。这个类型只描述目录布局，
    // 不检查文件是否存在，也不触发任何 IO 副作用。
    fn new(workspace_dir: PathBuf, cargo_target_dir: PathBuf) -> Self {
        Self {
            workspace_dir,
            cxx_build_dir: cargo_target_dir.join("cxx"),
            cargo_target_dir,
        }
    }

    fn cxx_build_dir(&self) -> &Path {
        &self.cxx_build_dir
    }

    fn state_dir(&self) -> PathBuf {
        self.cxx_build_dir.join(".state")
    }

    fn manifest_path(&self, build_type: BuildType) -> PathBuf {
        self.state_dir().join(format!("cxx-{}.json", build_type.label()))
    }

    fn cmake_output_dir(&self, build_type: BuildType) -> PathBuf {
        self.cxx_build_dir.join("output").join(build_type.cmake_output_dir())
    }

    fn cargo_output_dir(&self, build_type: BuildType) -> PathBuf {
        self.cargo_target_dir.join(build_type.cargo_output_dir())
    }

    fn compile_commands_source(&self) -> PathBuf {
        self.cxx_build_dir.join("clang-cl").join("Debug").join("compile_commands.json")
    }

    fn compile_commands_cxx_copy(&self) -> PathBuf {
        self.cxx_build_dir.join("compile_commands.json")
    }

    fn compile_commands_vscode_copy(&self) -> PathBuf {
        self.workspace_dir.join(".vscode").join("compile_commands.json")
    }
}

struct CxxBuildRunner {
    layout: CxxBuildLayout,
    cxx_project_dir: PathBuf,
    cmake_preset: visual_studio::CmakePreset,
    force: bool,
    packager: StreamlineRuntimePackager,
}

impl CxxBuildRunner {
    // Runner 是 cxx-build 的顶层流程所有者：它决定一个 profile 是否需要重新
    // configure/build，以及何时刷新部署副本和 manifest。具体“扫描哪些文件”
    // 与“复制哪些 runtime”下放给更窄的类型，避免主流程夹杂目录遍历细节。
    fn new(
        layout: CxxBuildLayout,
        cxx_project_dir: PathBuf,
        cmake_preset: visual_studio::CmakePreset,
        force: bool,
    ) -> Self {
        Self {
            layout,
            cxx_project_dir,
            cmake_preset,
            force,
            packager: StreamlineRuntimePackager,
        }
    }

    fn run(&self, profile: BuildProfile) -> Result<(), String> {
        for build_type in profile.build_types() {
            self.run_profile(*build_type)?;
        }
        Ok(())
    }

    fn run_profile(&self, build_type: BuildType) -> Result<(), String> {
        // 每个 profile 独立加载、比较和保存 manifest。Debug/Release 的 CMake preset、
        // native 输出目录和 Streamline runtime 来源都不同，不能共享同一份缓存判断。
        let manifest_path = self.layout.manifest_path(build_type);
        let previous_manifest = CxxProfileManifest::load(&manifest_path)?;
        let input = CxxInputSnapshot::capture(
            &self.layout.workspace_dir,
            &self.cxx_project_dir,
            &self.cmake_preset,
            build_type,
        )?;

        // 增量复用只要求“输入快照一致 + CMake 侧产物仍存在”。
        // Cargo 运行目录中的 DLL/json 不参与这个判断：它们只是可执行文件旁边的部署副本，
        // 可能被用户手动删除，也可能因为调试进程退出后需要恢复。把部署副本也纳入
        // CMake 复用条件会导致“只缺一个 DLL”时重新 configure/build，反而破坏最小编译目标。
        let can_reuse_cmake_outputs = !self.force
            && previous_manifest.as_ref().is_some_and(|manifest| {
                manifest.input == input && manifest.cmake_outputs_exist(&self.layout.workspace_dir)
            });

        if can_reuse_cmake_outputs {
            log::info!("CXX {} inputs unchanged; skip CMake configure/build.", build_type.label());
            let copy_report = self.packager.copy_to_rust(&self.layout, build_type, previous_manifest.as_ref())?;
            CxxProfileManifest::new(input, copy_report).save(&manifest_path)?;
            return Ok(());
        }

        if self.force {
            self.clean_cmake_output(build_type)?;
        }

        self.run_cmake(&["--preset", self.cmake_preset.configure], "configure")?;
        if let Err(err) = self.sync_compile_commands() {
            log::warn!("Skip compile_commands.json sync: {err}");
        }

        let build_preset = match build_type {
            BuildType::Debug => self.cmake_preset.build_debug,
            BuildType::Release => self.cmake_preset.build_release,
        };
        self.run_cmake(&["--build", "--preset", build_preset], &format!("build {}", build_type.label()))?;

        let copy_report = self.packager.copy_to_rust(&self.layout, build_type, previous_manifest.as_ref())?;
        CxxProfileManifest::new(input, copy_report).save(&manifest_path)?;
        Ok(())
    }

    fn run_cmake(&self, args: &[&str], action: &str) -> Result<(), String> {
        log::info!("Run cmake {}: cmake {}", action, args.join(" "));

        let status = std::process::Command::new("cmake")
            .current_dir(&self.cxx_project_dir)
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

    /// 清理 CMake 输出目录只用于显式 force，普通增量路径必须保留 CMake 自身的 object cache。
    fn clean_cmake_output(&self, build_type: BuildType) -> Result<(), String> {
        let cmake_output_path = self.layout.cmake_output_dir(build_type);
        if !cmake_output_path.exists() {
            return Ok(());
        }

        log::info!("Force clean CMake output dir: {}", cmake_output_path.display());
        std::fs::remove_dir_all(&cmake_output_path)
            .map_err(|err| format!("无法清理 CMake 输出目录 {}: {err}", cmake_output_path.display()))
    }
}

struct CmakeOutputScanner<'a> {
    layout: &'a CxxBuildLayout,
    build_type: BuildType,
}

impl<'a> CmakeOutputScanner<'a> {
    // Scanner 只读取 CMake 已经生成的输出，不负责触发 CMake，也不负责部署到 Cargo。
    // 这个边界保证 manifest 的“CMake 产物是否还在”和部署逻辑可以独立演进。
    fn new(layout: &'a CxxBuildLayout, build_type: BuildType) -> Self {
        Self { layout, build_type }
    }

    fn native_artifacts(&self) -> Result<Vec<PathBuf>, String> {
        let cmake_output_path = self.layout.cmake_output_dir(self.build_type);
        let mut source_paths = Vec::new();
        for entry in std::fs::read_dir(&cmake_output_path)
            .map_err(|err| format!("无法读取 CMake 输出目录 {}: {err}", cmake_output_path.display()))?
        {
            let entry =
                entry.map_err(|err| format!("无法读取 CMake 输出目录项 {}: {err}", cmake_output_path.display()))?;
            let source_path = entry.path();
            if Self::is_native_artifact(&source_path) {
                source_paths.push(source_path);
            }
        }
        source_paths.sort();
        Ok(source_paths)
    }

    fn output_stamps(&self) -> Result<Vec<FileStamp>, String> {
        let mut paths = self.native_artifacts()?;

        // CMake 的 GenerateExportHeader 会写回 C API include 目录。Rust bindgen 依赖这些 header，
        // 因此 manifest 把它们视为 CMake 产物：若被手动删除，下次不会误判为可完全跳过。
        paths.extend([
            self.layout
                .workspace_dir
                .join("engine/cxx/mods/truvixx-assimp/include/TruvixxAssimp/c_api/truvixx_assimp.export.h"),
            self.layout
                .workspace_dir
                .join("engine/cxx/mods/truvixx-streamline/include/TruvixxStreamline/c_api/truvixx_streamline.export.h"),
        ]);

        paths
            .into_iter()
            .filter(|path| path.is_file())
            .map(|path| FileStamp::from_path(&self.layout.workspace_dir, &path))
            .collect()
    }

    fn is_native_artifact(path: &Path) -> bool {
        path.extension().is_some_and(|suffix| suffix == "dll" || suffix == "pdb" || suffix == "lib")
    }
}

struct CargoRuntimeDeployment<'a> {
    workspace_dir: &'a Path,
    cargo_output_path: PathBuf,
    managed_outputs: BTreeSet<String>,
    copied_files: Vec<String>,
}

impl<'a> CargoRuntimeDeployment<'a> {
    // Deployment 持有一次部署过程的可变状态：本轮声明管理的输出，以及实际发生复制的文件名。
    // managed_outputs 是清理边界，copied_files 只是日志信息；二者分开可以避免为了日志而扩大删除范围。
    fn new(layout: &'a CxxBuildLayout, build_type: BuildType) -> Self {
        Self {
            workspace_dir: &layout.workspace_dir,
            cargo_output_path: layout.cargo_output_dir(build_type),
            managed_outputs: BTreeSet::new(),
            copied_files: Vec::new(),
        }
    }

    fn ensure_output_dirs(&self) -> Result<(), String> {
        std::fs::create_dir_all(&self.cargo_output_path)
            .map_err(|err| format!("无法创建 Cargo 输出目录 {}: {err}", self.cargo_output_path.display()))?;
        std::fs::create_dir_all(self.cargo_output_path.join("examples")).map_err(|err| {
            format!("无法创建 Cargo examples 输出目录 {}: {err}", self.cargo_output_path.join("examples").display())
        })
    }

    fn output_dirs(&self) -> [PathBuf; 2] {
        // Cargo bin 和 examples 都可能直接启动可执行文件，因此 native DLL 与 Streamline
        // JSON 必须保持同一套部署规则。这里集中返回两个装载边界，避免调用方漏掉 examples。
        [self.cargo_output_path.clone(), self.cargo_output_path.join("examples")]
    }

    fn copy_file_to_outputs_if_changed(&mut self, source_path: &Path) -> Result<(), String> {
        let file_name = source_path.file_name().ok_or_else(|| format!("无法获取文件名: {}", source_path.display()))?;
        let destinations = [
            self.cargo_output_path.join(file_name),
            self.cargo_output_path.join("examples").join(file_name),
        ];

        for destination_path in destinations {
            // 即使目标文件内容没有变化，也要把路径登记到 managed_outputs：
            // stale cleanup 关心的是“本轮仍由工具管理”，不是“本轮是否真的复制”。
            let relative_path = CxxBuildFileHelper::relative_slash_path(self.workspace_dir, &destination_path);
            self.managed_outputs.insert(relative_path);
            if CxxBuildFileHelper::copy_if_changed_to_path(source_path, &destination_path)? {
                self.copied_files.push(
                    destination_path
                        .file_name()
                        .expect("destination should have file name")
                        .to_string_lossy()
                        .to_string(),
                );
            }
        }

        Ok(())
    }

    fn remove_stale_previous_outputs(&self, previous_manifest: Option<&CxxProfileManifest>) -> Result<(), String> {
        let Some(previous_manifest) = previous_manifest else {
            return Ok(());
        };

        // stale cleanup 以“旧 manifest 管理过的部署副本”为删除上限。这个边界很重要：
        // build/{profile} 同时也是 Cargo、开发工具和用户临时文件可能出现的位置，不能
        // 因为 native target 列表变化就扫描目录后按后缀批量删除。
        for old_output in &previous_manifest.cargo_outputs {
            if self.managed_outputs.contains(&old_output.path) {
                continue;
            }

            Self::remove_file_if_exists(&self.workspace_dir.join(old_output.path.replace('/', "\\")))?;
        }

        Ok(())
    }

    fn output_stamps(&self) -> Result<Vec<FileStamp>, String> {
        self.managed_outputs
            .iter()
            .map(|relative_path| self.workspace_dir.join(relative_path.replace('/', "\\")))
            .filter(|path| path.is_file())
            .map(|path| FileStamp::from_path(self.workspace_dir, &path))
            .collect()
    }

    fn copied_files(&self) -> &[String] {
        &self.copied_files
    }

    fn remove_file_if_exists(path: &Path) -> Result<(), String> {
        if !path.exists() {
            return Ok(());
        }

        std::fs::remove_file(path).map_err(|err| format!("无法删除旧文件 {}: {err}", path.display()))
    }
}

struct StreamlineRuntimePackager;

impl StreamlineRuntimePackager {
    // Packager 只负责 Streamline 与 native runtime 的部署策略：哪些文件必须存在、
    // Debug/Release 的 runtime 来源有什么差异、哪些旧文件允许被清理。它不直接
    // 持有部署状态，避免把一次 copy 的临时集合泄漏成长期对象状态。
    fn copy_to_rust(
        &self,
        layout: &CxxBuildLayout,
        build_type: BuildType,
        previous_manifest: Option<&CxxProfileManifest>,
    ) -> Result<CxxCopyReport, String> {
        let cmake_outputs = CmakeOutputScanner::new(layout, build_type);
        let mut deployment = CargoRuntimeDeployment::new(layout, build_type);

        // Cargo 运行目录和 examples 目录都是 native DLL 的装载边界；复制逻辑必须同时维护两处，
        // 否则 `cargo run --bin` 与 sample/example 启动时看到的 runtime 集合会不一致。
        deployment.ensure_output_dirs()?;

        // managed_outputs 只记录本工具明确拥有的部署副本。后续 stale cleanup 只会删除这些
        // 由旧 manifest 证明曾被本工具管理的文件，避免误删用户临时放在 build/debug 里的其它 DLL。
        for source_path in cmake_outputs.native_artifacts()? {
            deployment.copy_file_to_outputs_if_changed(&source_path)?;
        }

        // CMake 输出、Streamline runtime 和 Streamline JSON 是三类不同来源：
        // - CMake 输出来自当前 profile 的 native build；
        // - Streamline runtime 来自 SDK 的 debug/development 或 release/production 目录；
        // - JSON config 来自项目维护的 tools/streamline 模板。
        // 三者都必须部署到 executable 同级目录，但它们的输入来源和失效条件不能混在一起。
        let runtime_sources = self.streamline_runtime_sources(build_type)?;
        for source_path in runtime_sources {
            deployment.copy_file_to_outputs_if_changed(&source_path)?;
        }

        let config_sources = self.streamline_json_configs()?;
        let config_names = config_sources
            .iter()
            .filter_map(|path| path.file_name().map(|name| name.to_string_lossy().to_string()))
            .collect::<BTreeSet<_>>();

        self.clean_disallowed_streamline_runtime(&deployment, build_type, &config_names)?;
        for source_path in config_sources {
            deployment.copy_file_to_outputs_if_changed(&source_path)?;
        }

        // 旧 manifest 是“上一轮本工具管理过什么”的边界。只有旧 manifest 中出现、
        // 但本轮 managed_outputs 不再声明的文件才算 stale；这样可以清掉被移除的 target
        // 或 runtime 文件，又不会把 Cargo 输出目录当成可随意清空的临时目录。
        deployment.remove_stale_previous_outputs(previous_manifest)?;

        let cmake_outputs = cmake_outputs.output_stamps()?;
        let cargo_outputs = deployment.output_stamps()?;

        if deployment.copied_files().is_empty() {
            log::info!("CXX {} Cargo runtime already up to date.", build_type.label());
        } else {
            log::info!("Copied CXX {} files: {:#?}", build_type.label(), deployment.copied_files());
        }

        Ok(CxxCopyReport {
            cmake_outputs,
            cargo_outputs,
        })
    }

    fn streamline_runtime_sources(&self, build_type: BuildType) -> Result<Vec<PathBuf>, String> {
        // Streamline runtime 是运行时依赖，不是 CMake target 产物。这里单独解析 SDK
        // 目录并做缺失检查，避免把第三方 DLL 的存在性错误伪装成 CMake 构建错误。
        let streamline_sdk_root = TruvisPath::tools_path().join("streamline-sdk");
        if !streamline_sdk_root.exists() {
            return Err(format!(
                "Streamline SDK 不存在: {}。请先运行 `just fetch-res`。",
                streamline_sdk_root.display()
            ));
        }

        let runtime_dir = build_type.streamline_runtime_dir(&streamline_sdk_root);
        if !runtime_dir.exists() {
            return Err(format!("Streamline runtime 目录不存在: {}", runtime_dir.display()));
        }

        let mut source_paths = Vec::new();
        for dll_name in STREAMLINE_REQUIRED_DLLS {
            let source_path = runtime_dir.join(dll_name);
            if !source_path.exists() {
                return Err(format!("缺少 Streamline runtime 文件: {}", source_path.display()));
            }

            source_paths.push(source_path);
        }

        if matches!(build_type, BuildType::Debug) {
            for dll_name in STREAMLINE_DEBUG_OPTIONAL_DLLS {
                let source_path = runtime_dir.join(dll_name);
                if !source_path.exists() {
                    log::warn!("Skip optional Streamline debug runtime: {}", source_path.display());
                    continue;
                }

                source_paths.push(source_path);
            }
        }

        Ok(source_paths)
    }

    fn streamline_json_configs(&self) -> Result<Vec<PathBuf>, String> {
        let configs_dir = TruvisPath::tools_path().join("streamline");
        if !configs_dir.exists() {
            return Err(format!(
                "Streamline 配置目录不存在: {}。请提交 tools/streamline 下的 sl.*.json 模板。",
                configs_dir.display()
            ));
        }

        let mut config_paths = Vec::new();
        for entry in std::fs::read_dir(&configs_dir)
            .map_err(|err| format!("无法读取 Streamline 配置目录 {}: {err}", configs_dir.display()))?
        {
            let entry = entry.map_err(|err| format!("无法读取 Streamline 配置目录项: {err}"))?;
            let file_name = entry.file_name();
            let file_name = file_name.to_string_lossy();
            if Self::is_streamline_json_file_name(&file_name) {
                config_paths.push(entry.path());
            }
        }
        config_paths.sort();

        if config_paths.is_empty() {
            return Err(format!("Streamline 配置目录 {} 中没有 sl.*.json 文件", configs_dir.display()));
        }

        Ok(config_paths)
    }

    fn clean_disallowed_streamline_runtime(
        &self,
        deployment: &CargoRuntimeDeployment<'_>,
        build_type: BuildType,
        config_names: &BTreeSet<String>,
    ) -> Result<(), String> {
        for dir in deployment.output_dirs() {
            if !dir.exists() {
                continue;
            }

            // 这里只清理明确由本工具管理或历史上误管理过的 Streamline 文件。
            // Cargo 输出目录可能被开发者临时放入其它 DLL，不能按后缀或 sl.* 前缀粗暴删除。
            for entry in
                std::fs::read_dir(&dir).map_err(|err| format!("无法读取 Cargo 输出目录 {}: {err}", dir.display()))?
            {
                let entry = entry.map_err(|err| format!("无法读取 Cargo 输出目录项 {}: {err}", dir.display()))?;
                let file_name = entry.file_name();
                let file_name = file_name.to_string_lossy().to_string();

                let is_removed =
                    STREAMLINE_REMOVED_MANAGED_DLLS.iter().any(|name| name.eq_ignore_ascii_case(&file_name));
                let is_release_debug_optional = matches!(build_type, BuildType::Release)
                    && STREAMLINE_DEBUG_OPTIONAL_DLLS.iter().any(|name| name.eq_ignore_ascii_case(&file_name));
                let is_stale_streamline_json =
                    Self::is_streamline_json_file_name(&file_name) && !config_names.contains(&file_name);

                if is_removed || is_release_debug_optional || is_stale_streamline_json {
                    CargoRuntimeDeployment::remove_file_if_exists(&entry.path())?;
                }
            }
        }

        Ok(())
    }

    fn is_streamline_json_file_name(file_name: &str) -> bool {
        file_name.starts_with("sl.") && file_name.ends_with(".json")
    }
}

#[derive(Debug)]
struct CxxCopyReport {
    cmake_outputs: Vec<FileStamp>,
    cargo_outputs: Vec<FileStamp>,
}

// 每个 profile 单独保存 manifest，因为 Debug/Release 的 CMake preset、输出目录和
// Streamline runtime 来源都不同。manifest 只用于判断当前 profile 能否跳过 CMake；
// 它不是跨 profile 共享的全局缓存，也不承担清理整个 build 目录的职责。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct CxxProfileManifest {
    version: u32,
    input: CxxInputSnapshot,
    cmake_outputs: Vec<FileStamp>,
    cargo_outputs: Vec<FileStamp>,
}

impl CxxProfileManifest {
    // manifest 记录的是“是否可以跳过 CMake 调用”的证据，而不是完整构建目录索引。
    // cmake_outputs 用于防止 native 产物被手动删除后误判可复用；cargo_outputs 用于
    // 限定 stale cleanup 的删除上限，避免误删用户放在 build/{profile} 下的临时文件。
    fn new(input: CxxInputSnapshot, copy_report: CxxCopyReport) -> Self {
        Self {
            version: MANIFEST_VERSION,
            input,
            cmake_outputs: copy_report.cmake_outputs,
            cargo_outputs: copy_report.cargo_outputs,
        }
    }

    fn load(path: &Path) -> Result<Option<Self>, String> {
        if !path.is_file() {
            return Ok(None);
        }

        let content =
            std::fs::read_to_string(path).map_err(|err| format!("无法读取 CXX manifest {}: {err}", path.display()))?;
        let manifest: Self =
            serde_json::from_str(&content).map_err(|err| format!("无法解析 CXX manifest {}: {err}", path.display()))?;
        if manifest.version == MANIFEST_VERSION { Ok(Some(manifest)) } else { Ok(None) }
    }

    fn save(&self, path: &Path) -> Result<(), String> {
        let content = serde_json::to_string_pretty(self)
            .map_err(|err| format!("无法序列化 CXX manifest {}: {err}", path.display()))?;
        CxxBuildFileHelper::write_if_changed(path, content.as_bytes())
    }

    fn cmake_outputs_exist(&self, workspace_dir: &Path) -> bool {
        self.cmake_outputs.iter().all(|stamp| {
            let path = workspace_dir.join(stamp.path.replace('/', "\\"));
            path.is_file()
        })
    }
}

// 输入快照记录“会改变 native build 语义”的稳定字段。preset/profile/env/files 任一变化，
// 都应交回 CMake 自己做增量判断；本工具只负责决定是否可以完全跳过这次 CMake 调用。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct CxxInputSnapshot {
    profile: String,
    visual_studio_name: String,
    configure_preset: String,
    build_preset: String,
    env: Vec<EnvStamp>,
    files: Vec<FileStamp>,
}

impl CxxInputSnapshot {
    fn capture(
        workspace_dir: &Path,
        cxx_project_dir: &Path,
        cmake_preset: &visual_studio::CmakePreset,
        build_type: BuildType,
    ) -> Result<Self, String> {
        let build_preset = match build_type {
            BuildType::Debug => cmake_preset.build_debug,
            BuildType::Release => cmake_preset.build_release,
        };

        let files = CxxInputCollector::new(workspace_dir, cxx_project_dir).collect()?;

        Ok(Self {
            profile: build_type.label().to_string(),
            visual_studio_name: cmake_preset.visual_studio_name.to_string(),
            configure_preset: cmake_preset.configure.to_string(),
            build_preset: build_preset.to_string(),
            env: vec![EnvStamp::capture("VCPKG_ROOT")],
            files,
        })
    }
}

struct CxxInputCollector<'a> {
    workspace_dir: &'a Path,
    cxx_project_dir: &'a Path,
}

impl<'a> CxxInputCollector<'a> {
    // Collector 是输入快照的唯一来源。它只收集会改变 native build 语义的文件，
    // 不把 CMake 中间目录、Cargo 部署副本或其它运行时输出混进输入集合。
    fn new(workspace_dir: &'a Path, cxx_project_dir: &'a Path) -> Self {
        Self {
            workspace_dir,
            cxx_project_dir,
        }
    }

    fn collect(&self) -> Result<Vec<FileStamp>, String> {
        let mut inputs = Vec::new();
        // CXX manifest 是 profile 级增量契约，不是通用文件缓存。这里刻意只记录会影响
        // CMake 生成结果或 FFI ABI 的输入：preset、vcpkg manifest、C++ 源/头文件和
        // Streamline SDK 头文件。Cargo 输出目录、CMake 中间目录和部署副本都不是输入。
        for file_name in ["CMakeLists.txt", "CMakePresets.json", "vcpkg.json"] {
            self.push_file_if_exists(&mut inputs, &self.cxx_project_dir.join(file_name))?;
        }

        for root in [
            self.cxx_project_dir.join("mods"),
            TruvisPath::tools_path().join("streamline-sdk").join("include"),
        ] {
            self.collect_tree_inputs(&mut inputs, &root)?;
        }

        inputs.sort_by(|left, right| left.path.cmp(&right.path));
        Ok(inputs)
    }

    fn push_file_if_exists(&self, inputs: &mut Vec<FileStamp>, path: &Path) -> Result<(), String> {
        if path.is_file() {
            inputs.push(FileStamp::from_path(self.workspace_dir, path)?);
        }
        Ok(())
    }

    fn collect_tree_inputs(&self, inputs: &mut Vec<FileStamp>, root: &Path) -> Result<(), String> {
        if !root.exists() {
            return Ok(());
        }

        for entry in walkdir::WalkDir::new(root).into_iter().filter_map(Result::ok) {
            let path = entry.path();
            if !path.is_file() || Self::is_generated_cxx_file(path) {
                continue;
            }

            inputs.push(FileStamp::from_path(self.workspace_dir, path)?);
        }

        Ok(())
    }

    fn is_generated_cxx_file(path: &Path) -> bool {
        // GenerateExportHeader 会把 *.export.h 写回 include 目录。它们是 CMake 产物，
        // 不是人工维护的输入；把它们纳入输入会让每次 configure 后的时间戳变化反过来
        // 触发下一次 configure/build。
        path.file_name().is_some_and(|name| name.to_string_lossy().ends_with(".export.h"))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct EnvStamp {
    key: String,
    value: Option<String>,
}

impl EnvStamp {
    fn capture(key: &str) -> Self {
        Self {
            key: key.to_string(),
            value: std::env::var(key).ok(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
struct FileStamp {
    path: String,
    len: u64,
    modified_ms: u64,
}

impl FileStamp {
    // FileStamp 使用 workspace-relative slash path，保证 manifest 在 Windows 路径分隔符
    // 上稳定可比；len + modified_ms 是轻量级增量判断，不试图做内容 hash。
    fn from_path(workspace_dir: &Path, path: &Path) -> Result<Self, String> {
        let metadata =
            std::fs::metadata(path).map_err(|err| format!("无法读取文件元数据 {}: {err}", path.display()))?;
        Ok(Self {
            path: CxxBuildFileHelper::relative_slash_path(workspace_dir, path),
            len: metadata.len(),
            modified_ms: CxxBuildFileHelper::file_modified_ms(&metadata)?,
        })
    }
}

struct CxxBuildFileHelper;

impl CxxBuildFileHelper {
    // 这个 helper struct 只收纳无状态文件系统小工具，满足“helper 有归属”的要求。
    // 它不保存路径或缓存，也不引入 trait 抽象；有上下文和生命周期的逻辑仍放在上面的职责类型里。
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
        // 复制判断刻意保持便宜：目标不存在、长度不同，或源文件修改时间更新时才复制。
        // 这里不做 hash，避免每次 cxx-build 都读取大 DLL/PDB 的完整内容。
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

        Ok(Self::file_modified_ms(&source_metadata)? > Self::file_modified_ms(&destination_metadata)?)
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

    fn file_modified_ms(metadata: &std::fs::Metadata) -> Result<u64, String> {
        let modified = metadata.modified().map_err(|err| format!("无法读取文件修改时间: {err}"))?;
        let millis = modified.duration_since(UNIX_EPOCH).unwrap_or_default().as_millis();
        Ok(millis.min(u128::from(u64::MAX)) as u64)
    }

    fn relative_slash_path(root: &Path, path: &Path) -> String {
        path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/")
    }
}

fn main() -> Result<(), String> {
    TruvisLogger::init_with_file(LogFilePath::current_exe(TruvisPath::temp_dir()));

    let options = CliOptions::parse()?;
    let workspace_dir = TruvisPath::workspace_path();
    log::info!("workspace_dir: {:?}", workspace_dir);

    let target_dir = TruvisPath::target_path();
    log::info!("target_dir: {:?}", target_dir);

    let layout = CxxBuildLayout::new(workspace_dir, target_dir);
    log::info!("cxx_build_dir: {:?}", layout.cxx_build_dir());

    let cxx_project_dir = TruvisPath::cxx_root_path();
    log::info!("cxx_project_dir: {:?}", cxx_project_dir);

    let cmake_preset = visual_studio::select_cmake_preset()?;
    log::info!(
        "Using {} CMake presets: {}, {}, {}",
        cmake_preset.visual_studio_name,
        cmake_preset.configure,
        cmake_preset.build_debug,
        cmake_preset.build_release
    );

    let runner = CxxBuildRunner::new(layout, cxx_project_dir, cmake_preset, options.force);
    runner.run(options.profile)
}
