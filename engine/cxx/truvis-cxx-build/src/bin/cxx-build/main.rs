mod visual_studio;

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

// 第一阶段只接 DLSS Super Resolution，所以只复制 SL core + DLSS SR 所需 DLL。
// NvLowLatencyVk.dll 是 SL Vulkan backend 启动时加载的低延迟 helper，不表示启用 Reflex feature。
// Frame Generation、Ray Reconstruction、Reflex、NIS、DirectSR 等 feature 的 DLL 不在这里出现；
// 后续启用新 feature 时，应先扩展 C++ wrapper 的 featuresToLoad，再同步扩展这个清单。
const STREAMLINE_REQUIRED_DLLS: &[&str] = &[
    "sl.interposer.dll",
    "sl.common.dll",
    "sl.pcl.dll",
    "sl.dlss.dll",
    "nvngx_dlss.dll",
    "NvLowLatencyVk.dll",
];

// Debug runtime 可能依赖 PIX runtime。该 DLL 在 development 目录中存在时复制，不存在时跳过。
// `sl.imgui.dll` 虽然也在 development 目录中，但当前没有接 Streamline debug UI，因此不复制。
const STREAMLINE_DEBUG_OPTIONAL_DLLS: &[&str] = &["WinPixEventRuntime.dll"];

fn run_cmake(cmake_project: &std::path::Path, args: &[&str], action: &str) -> Result<(), String> {
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
fn clean_cmake_output(cmake_project: &std::path::Path, build_type: BuildType) -> Result<(), String> {
    let cmake_output_path = cmake_project.join("build").join("output").join(build_type.cmake_output_dir());
    if !cmake_output_path.exists() {
        return Ok(());
    }

    log::info!("Clean CMake output dir: {}", cmake_output_path.display());
    std::fs::remove_dir_all(&cmake_output_path)
        .map_err(|err| format!("无法清理 CMake 输出目录 {}: {err}", cmake_output_path.display()))
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
            let is_streamline_json = is_streamline_json_file_name(&file_name);

            if !is_required_dll && !is_debug_optional_dll && !is_streamline_json {
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

fn copy_file_to_cargo_outputs(
    source_path: &std::path::Path,
    cargo_output_path: &std::path::Path,
) -> Result<(), String> {
    let file_name = source_path.file_name().ok_or_else(|| format!("无法获取文件名: {}", source_path.display()))?;

    std::fs::copy(source_path, cargo_output_path.join(file_name))
        .map_err(|err| format!("无法复制 {} 到 {}: {err}", source_path.display(), cargo_output_path.display()))?;
    std::fs::copy(source_path, cargo_output_path.join("examples").join(file_name)).map_err(|err| {
        format!("无法复制 {} 到 {}: {err}", source_path.display(), cargo_output_path.join("examples").display())
    })?;

    Ok(())
}

fn copy_streamline_json_configs(cargo_output_path: &std::path::Path) -> Result<Vec<String>, String> {
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

fn copy_streamline_runtime(cargo_output_path: &std::path::Path, build_type: BuildType) -> Result<Vec<String>, String> {
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
fn copy_to_rust(
    cmake_project: &std::path::Path,
    cargo_target_dir: &std::path::Path,
    build_type: BuildType,
) -> Result<(), String> {
    let cmake_output_path = cmake_project.join("build").join("output").join(build_type.cmake_output_dir());
    let cargo_output_path = cargo_target_dir.join(build_type.cargo_output_dir());

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
    clean_cmake_output(&cxx_project_dir, BuildType::Debug)?;
    clean_cmake_output(&cxx_project_dir, BuildType::Release)?;
    run_cmake(&cxx_project_dir, &["--build", "--preset", cmake_preset.build_debug], "build debug")?;
    run_cmake(&cxx_project_dir, &["--build", "--preset", cmake_preset.build_release], "build release")?;

    copy_to_rust(&cxx_project_dir, &target_dir, BuildType::Debug)?;
    copy_to_rust(&cxx_project_dir, &target_dir, BuildType::Release)?;

    Ok(())
}
