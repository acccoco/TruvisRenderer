mod visual_studio;

use std::path::{Path, PathBuf};

use truvis_logs::init_log;
use truvis_path::TruvisPath;

#[derive(Clone, Copy)]
enum BuildType {
    Debug,
    Release,
}
impl BuildType {
    fn cmake_output_dir(&self) -> &str {
        match self {
            BuildType::Debug => "Debug",
            BuildType::Release => "Release",
        }
    }
    fn cargo_output_dir(&self) -> &str {
        match self {
            BuildType::Debug => "debug",
            BuildType::Release => "release",
        }
    }

    fn streamline_runtime_dir(&self, streamline_sdk_root: &std::path::Path) -> std::path::PathBuf {
        // Streamline SDK 同时提供 production 和 development runtime：
        // - Debug 使用 development，便于 SL 输出更完整的诊断信息。
        // - Release 使用 production，避免把开发向 DLL 带入发布运行目录。
        match self {
            BuildType::Debug => streamline_sdk_root.join("bin").join("x64").join("development"),
            BuildType::Release => streamline_sdk_root.join("bin").join("x64"),
        }
    }
}

// 第一阶段只接 DLSS Super Resolution，所以 Release 只复制 SL core + DLSS SR 所需 DLL。
// NvLowLatencyVk.dll 是 SL Vulkan backend 启动时加载的低延迟 helper，不表示启用 Reflex feature。
// Frame Generation、Ray Reconstruction、Reflex、NIS、DirectSR 等 feature 的 DLL 不在这里出现；
// 后续启用新 feature 时，应先扩展 Rust 侧 feature flags，再同步扩展这个清单。
const STREAMLINE_REQUIRED_DLLS: &[&str] = &[
    "sl.interposer.dll",
    "sl.common.dll",
    "sl.pcl.dll",
    "sl.dlss.dll",
    "nvngx_dlss.dll",
    "NvLowLatencyVk.dll",
];

// Debug 使用 development runtime，并保留可由 TRUVIS_STREAMLINE_IMGUI 启用的 Streamline ImGui 调试 UI。
// sl.imgui 只进入 Debug 运行目录，Release 继续保持 DLSS SR 最小 runtime。
const STREAMLINE_DEBUG_OPTIONAL_DLLS: &[&str] = &["sl.imgui.dll"];

// 旧版本曾把 WinPixEventRuntime.dll 当作 Debug optional runtime 复制。
// 当前 Vulkan 路径不依赖 PIX runtime，保留清理项是为了删除已有构建目录中的旧残留。
const STREAMLINE_REMOVED_MANAGED_DLLS: &[&str] = &["WinPixEventRuntime.dll"];

struct CxxBuildLayout {
    workspace_dir: PathBuf,
    cxx_build_dir: PathBuf,
    cargo_target_dir: PathBuf,
}

impl CxxBuildLayout {
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

fn run_cmake(cmake_project: &Path, args: &[&str], action: &str) -> Result<(), String> {
    log::info!("Run cmake {}: cmake {}", action, args.join(" "));

    let status = std::process::Command::new("cmake")
        .current_dir(cmake_project)
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

/// 清理 CMake 输出目录，避免已经移除的 C++ target 残留产物继续被复制到 Cargo 输出目录。
fn clean_cmake_output(layout: &CxxBuildLayout, build_type: BuildType) -> Result<(), String> {
    let cmake_output_path = layout.cmake_output_dir(build_type);
    if !cmake_output_path.exists() {
        return Ok(());
    }

    log::info!("Clean CMake output dir: {}", cmake_output_path.display());
    std::fs::remove_dir_all(&cmake_output_path)
        .map_err(|err| format!("无法清理 CMake 输出目录 {}: {err}", cmake_output_path.display()))
}

fn copy_file_to_path(source_path: &Path, destination_path: &Path) -> Result<(), String> {
    let parent =
        destination_path.parent().ok_or_else(|| format!("无法获取目标目录: {}", destination_path.display()))?;
    std::fs::create_dir_all(parent).map_err(|err| format!("无法创建目录 {}: {err}", parent.display()))?;
    std::fs::copy(source_path, destination_path)
        .map_err(|err| format!("无法复制 {} 到 {}: {err}", source_path.display(), destination_path.display()))?;

    Ok(())
}

fn sync_compile_commands(cmake_project: &Path, layout: &CxxBuildLayout) -> Result<(), String> {
    run_cmake(cmake_project, &["--preset", "clang-cl-debug"], "configure compile_commands")?;

    let source_path = layout.compile_commands_source();
    if !source_path.is_file() {
        return Err(format!("clang-cl-debug preset 没有生成 compile_commands.json: {}", source_path.display()));
    }

    let cxx_copy_path = layout.compile_commands_cxx_copy();
    let vscode_copy_path = layout.compile_commands_vscode_copy();
    copy_file_to_path(&source_path, &cxx_copy_path)?;
    copy_file_to_path(&source_path, &vscode_copy_path)?;

    log::info!("Synced compile_commands.json to {} and {}", cxx_copy_path.display(), vscode_copy_path.display());
    Ok(())
}

/// 清理 Cargo 输出目录里的旧 Truvis C++ 产物，确保移除的 C++ target 不会以陈旧 DLL/lib 形式残留。
fn clean_cargo_cxx_artifacts(cargo_output_path: &std::path::Path) {
    let dirs = [cargo_output_path.to_path_buf(), cargo_output_path.join("examples")];
    for dir in dirs {
        if !dir.exists() {
            continue;
        }

        for entry in std::fs::read_dir(&dir).unwrap() {
            let entry = entry.unwrap();
            let file_name = entry.file_name();
            let file_name = file_name.to_string_lossy();
            if !file_name.starts_with("truvixx-") {
                continue;
            }

            let source_path = entry.path();
            let suffix = source_path.extension().unwrap_or_default();
            if suffix == "dll" || suffix == "pdb" || suffix == "lib" {
                std::fs::remove_file(source_path).unwrap();
            }
        }
    }
}

fn clean_managed_streamline_runtime(cargo_output_path: &std::path::Path) -> Result<(), String> {
    // 只清理本函数族管理的 Streamline runtime 文件，避免误删其他工具或用户临时放在
    // Cargo 输出目录里的 DLL。这里包括固定 DLL 清单和 tools/streamline 里的 sl.*.json。
    let dirs = [cargo_output_path.to_path_buf(), cargo_output_path.join("examples")];
    for dir in dirs {
        if !dir.exists() {
            continue;
        }

        for entry in
            std::fs::read_dir(&dir).map_err(|err| format!("无法读取 Cargo 输出目录 {}: {err}", dir.display()))?
        {
            let entry = entry.map_err(|err| format!("无法读取 Cargo 输出目录项 {}: {err}", dir.display()))?;
            let file_name = entry.file_name();
            let file_name = file_name.to_string_lossy();

            let is_required_dll = STREAMLINE_REQUIRED_DLLS.iter().any(|name| name.eq_ignore_ascii_case(&file_name));
            let is_debug_optional_dll =
                STREAMLINE_DEBUG_OPTIONAL_DLLS.iter().any(|name| name.eq_ignore_ascii_case(&file_name));
            let is_removed_managed_dll =
                STREAMLINE_REMOVED_MANAGED_DLLS.iter().any(|name| name.eq_ignore_ascii_case(&file_name));
            let is_streamline_json = is_streamline_json_file_name(&file_name);

            if !is_required_dll && !is_debug_optional_dll && !is_removed_managed_dll && !is_streamline_json {
                continue;
            }

            std::fs::remove_file(entry.path())
                .map_err(|err| format!("无法删除旧 Streamline runtime {}: {err}", entry.path().display()))?;
        }
    }

    Ok(())
}

fn is_streamline_json_file_name(file_name: &str) -> bool {
    file_name.starts_with("sl.") && file_name.ends_with(".json")
}

fn copy_file_to_cargo_outputs(source_path: &Path, cargo_output_path: &Path) -> Result<(), String> {
    let file_name = source_path.file_name().ok_or_else(|| format!("无法获取文件名: {}", source_path.display()))?;

    std::fs::copy(source_path, cargo_output_path.join(file_name))
        .map_err(|err| format!("无法复制 {} 到 {}: {err}", source_path.display(), cargo_output_path.display()))?;
    std::fs::copy(source_path, cargo_output_path.join("examples").join(file_name)).map_err(|err| {
        format!("无法复制 {} 到 {}: {err}", source_path.display(), cargo_output_path.join("examples").display())
    })?;

    Ok(())
}

fn copy_streamline_json_configs(cargo_output_path: &Path) -> Result<Vec<String>, String> {
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
        if !is_streamline_json_file_name(&file_name) {
            continue;
        }

        config_paths.push(entry.path());
    }
    config_paths.sort();

    if config_paths.is_empty() {
        return Err(format!("Streamline 配置目录 {} 中没有 sl.*.json 文件", configs_dir.display()));
    }

    let mut copied_files = Vec::new();
    for source_path in config_paths {
        copy_file_to_cargo_outputs(&source_path, cargo_output_path)?;
        copied_files.push(
            source_path
                .file_name()
                .expect("Streamline config path should have file name")
                .to_string_lossy()
                .to_string(),
        );
    }

    Ok(copied_files)
}

fn copy_streamline_runtime(cargo_output_path: &Path, build_type: BuildType) -> Result<Vec<String>, String> {
    let streamline_sdk_root = TruvisPath::tools_path().join("streamline-sdk");
    if !streamline_sdk_root.exists() {
        return Err(format!("Streamline SDK 不存在: {}。请先运行 `just fetch-res`。", streamline_sdk_root.display()));
    }

    let runtime_dir = build_type.streamline_runtime_dir(&streamline_sdk_root);
    if !runtime_dir.exists() {
        return Err(format!("Streamline runtime 目录不存在: {}", runtime_dir.display()));
    }

    clean_managed_streamline_runtime(cargo_output_path)?;

    // DLL 必须和最终 executable 位于同一目录。这样 Rust 侧通过 ash 加载 sl.interposer.dll，
    // C++ wrapper 通过运行时绝对路径加载同一份 sl.interposer.dll。
    let mut copied_files = Vec::new();
    for dll_name in STREAMLINE_REQUIRED_DLLS {
        let source_path = runtime_dir.join(dll_name);
        if !source_path.exists() {
            return Err(format!("缺少 Streamline runtime 文件: {}", source_path.display()));
        }

        copy_file_to_cargo_outputs(&source_path, cargo_output_path)?;
        copied_files.push((*dll_name).to_string());
    }

    if matches!(build_type, BuildType::Debug) {
        for dll_name in STREAMLINE_DEBUG_OPTIONAL_DLLS {
            let source_path = runtime_dir.join(dll_name);
            if !source_path.exists() {
                log::warn!("Skip optional Streamline debug runtime: {}", source_path.display());
                continue;
            }

            copy_file_to_cargo_outputs(&source_path, cargo_output_path)?;
            copied_files.push((*dll_name).to_string());
        }
    }

    copied_files.extend(copy_streamline_json_configs(cargo_output_path)?);

    Ok(copied_files)
}

/// 将 cxx 编译结果复制到 Rust 侧
fn copy_to_rust(layout: &CxxBuildLayout, build_type: BuildType) -> Result<(), String> {
    let cmake_output_path = layout.cmake_output_dir(build_type);
    let cargo_output_path = layout.cargo_output_dir(build_type);

    // 确保 Cargo 输出目录及其 examples 子目录存在；当前配置下是 build/{profile}。
    std::fs::create_dir_all(&cargo_output_path)
        .map_err(|err| format!("无法创建 Cargo 输出目录 {}: {err}", cargo_output_path.display()))?;
    std::fs::create_dir_all(cargo_output_path.join("examples"))
        .map_err(|err| format!("无法创建 Cargo examples 输出目录 {}: {err}", cargo_output_path.display()))?;

    clean_cargo_cxx_artifacts(&cargo_output_path);

    let mut all_copy_files = Vec::new();
    for entry in std::fs::read_dir(&cmake_output_path)
        .map_err(|err| format!("无法读取 CMake 输出目录 {}: {err}", cmake_output_path.display()))?
    {
        let entry = entry.map_err(|err| format!("无法读取 CMake 输出目录项 {}: {err}", cmake_output_path.display()))?;
        let file_name = entry.file_name();
        let source_path = entry.path();
        let suffix = source_path.extension().unwrap_or_default();

        // 需要复制的文件：.dll, .pdb, .lib
        if suffix != "dll" && suffix != "pdb" && suffix != "lib" {
            continue;
        }

        all_copy_files.push(file_name.to_str().unwrap().to_string());

        copy_file_to_cargo_outputs(&source_path, &cargo_output_path)?;
    }

    let streamline_files = copy_streamline_runtime(&cargo_output_path, build_type)?;
    all_copy_files.extend(streamline_files);

    log::info!("Copied files to {}: {:#?}", cargo_output_path.display(), all_copy_files);
    Ok(())
}

fn main() -> Result<(), String> {
    init_log();

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

    run_cmake(&cxx_project_dir, &["--preset", cmake_preset.configure], "configure")?;
    if let Err(err) = sync_compile_commands(&cxx_project_dir, &layout) {
        log::warn!("Skip compile_commands.json sync: {err}");
    }

    clean_cmake_output(&layout, BuildType::Debug)?;
    clean_cmake_output(&layout, BuildType::Release)?;
    run_cmake(&cxx_project_dir, &["--build", "--preset", cmake_preset.build_debug], "build debug")?;
    run_cmake(&cxx_project_dir, &["--build", "--preset", cmake_preset.build_release], "build release")?;

    copy_to_rust(&layout, BuildType::Debug)?;
    copy_to_rust(&layout, BuildType::Release)?;

    Ok(())
}
