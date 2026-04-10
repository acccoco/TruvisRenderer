use truvis_crate_tools::init_log::init_log;
use truvis_crate_tools::resource::TruvisPath;

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

/// 将 cxx 编译的结果 copy 到 rust
fn copy_to_rust(cmake_project: &std::path::Path, cargo_target_dir: &std::path::Path, build_type: BuildType) {
    let cmake_output_path = cmake_project.join("build").join("output").join(build_type.cmake_output_dir());
    let cargo_output_path = cargo_target_dir.join(build_type.cargo_output_dir());

    // 确保 target/debug, target/debug/examples 目录存在
    std::fs::create_dir_all(cargo_output_path.join("examples")).unwrap();
    // 确保 target/release, target/release/examples 目录存在
    std::fs::create_dir_all(cargo_output_path.join("examples")).unwrap();

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

fn main() {
    init_log();

    let workspace_dir = TruvisPath::workspace_path();
    log::info!("workspace_dir: {:?}", workspace_dir);

    let target_dir = TruvisPath::target_path();
    log::info!("target_dir: {:?}", target_dir);

    let cxx_project_dir = TruvisPath::cxx_root_path();
    log::info!("cxx_project_dir: {:?}", cxx_project_dir);

    std::process::Command::new("cmake")
        .current_dir(&cxx_project_dir)
        .args(["--preset", "vs2022"])
        .status()
        .expect("Failed to run cmake");

    std::process::Command::new("cmake")
        .current_dir(&cxx_project_dir)
        .args(["--build", "--preset", "vs2022-build-debug"])
        .status()
        .expect("Failed to run cmake build");
    std::process::Command::new("cmake")
        .current_dir(&cxx_project_dir)
        .args(["--build", "--preset", "vs2022-build-release"])
        .status()
        .expect("Failed to run cmake build");

    copy_to_rust(&cxx_project_dir, &target_dir, BuildType::Debug);
    copy_to_rust(&cxx_project_dir, &target_dir, BuildType::Release);
}
