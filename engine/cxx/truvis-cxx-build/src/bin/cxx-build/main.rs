mod visual_studio;

use truvis_logs::init_log;
use truvis_path::TruvisPath;

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
}

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

/// 清理 CMake 输出目录，避免已经移除的 C++ target 残留产物继续被复制到 Cargo target。
fn clean_cmake_output(cmake_project: &std::path::Path, build_type: BuildType) -> Result<(), String> {
    let cmake_output_path = cmake_project.join("build").join("output").join(build_type.cmake_output_dir());
    if !cmake_output_path.exists() {
        return Ok(());
    }

    log::info!("Clean CMake output dir: {}", cmake_output_path.display());
    std::fs::remove_dir_all(&cmake_output_path)
        .map_err(|err| format!("无法清理 CMake 输出目录 {}: {err}", cmake_output_path.display()))
}

/// 清理 Rust target 里的旧 Truvis C++ 产物，确保移除的 target 不会以陈旧 DLL/lib 形式残留。
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

/// 将 cxx 编译结果复制到 Rust 侧
fn copy_to_rust(cmake_project: &std::path::Path, cargo_target_dir: &std::path::Path, build_type: BuildType) {
    let cmake_output_path = cmake_project.join("build").join("output").join(build_type.cmake_output_dir());
    let cargo_output_path = cargo_target_dir.join(build_type.cargo_output_dir());

    // 确保 target/debug, target/debug/examples 目录存在
    std::fs::create_dir_all(cargo_output_path.join("examples")).unwrap();
    // 确保 target/release, target/release/examples 目录存在
    std::fs::create_dir_all(cargo_output_path.join("examples")).unwrap();

    clean_cargo_cxx_artifacts(&cargo_output_path);

    let mut all_copy_files = Vec::new();
    for entry in std::fs::read_dir(cmake_output_path).unwrap() {
        let entry = entry.unwrap();
        let file_name = entry.file_name();
        let source_path = entry.path();
        let suffix = source_path.extension().unwrap_or_default();

        // 需要复制的文件：.dll, .pdb, .lib
        if suffix != "dll" && suffix != "pdb" && suffix != "lib" {
            continue;
        }

        all_copy_files.push(file_name.to_str().unwrap().to_string());

        std::fs::copy(&source_path, cargo_output_path.join(&file_name)).unwrap();
        std::fs::copy(&source_path, cargo_output_path.join("examples").join(&file_name)).unwrap();
    }

    log::info!("Copied files to {}: {:#?}", cargo_output_path.display(), all_copy_files);
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

    copy_to_rust(&cxx_project_dir, &target_dir, BuildType::Debug);
    copy_to_rust(&cxx_project_dir, &target_dir, BuildType::Release);

    Ok(())
}
