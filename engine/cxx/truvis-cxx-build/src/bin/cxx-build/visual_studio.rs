use std::path::{Path, PathBuf};
use std::process::Command;

const MSVC_TOOLS_COMPONENT: &str = "Microsoft.VisualStudio.Component.VC.Tools.x86.x64";

#[derive(Clone, Copy, Debug)]
pub struct CmakePreset {
    pub visual_studio_name: &'static str,
    pub configure: &'static str,
    pub build_debug: &'static str,
    pub build_release: &'static str,
}

#[derive(Clone, Copy)]
struct VisualStudioCandidate {
    name: &'static str,
    version_range: &'static str,
    generator: &'static str,
    configure: &'static str,
    build_debug: &'static str,
    build_release: &'static str,
}

const VISUAL_STUDIO_CANDIDATES: &[VisualStudioCandidate] = &[
    VisualStudioCandidate {
        name: "Visual Studio 2026",
        version_range: "[18.0,19.0)",
        generator: "Visual Studio 18 2026",
        configure: "vs2026",
        build_debug: "vs2026-build-debug",
        build_release: "vs2026-build-release",
    },
    VisualStudioCandidate {
        name: "Visual Studio 2022",
        version_range: "[17.0,18.0)",
        generator: "Visual Studio 17 2022",
        configure: "vs2022",
        build_debug: "vs2022-build-debug",
        build_release: "vs2022-build-release",
    },
];

pub fn select_cmake_preset() -> Result<CmakePreset, String> {
    let vswhere = find_vswhere().ok_or_else(|| {
        "找不到 vswhere.exe，无法检测 Visual Studio。请确认已安装 Visual Studio Installer。".to_string()
    })?;

    for candidate in VISUAL_STUDIO_CANDIDATES {
        if !has_msvc_tools(&vswhere, candidate)? {
            continue;
        }

        ensure_cmake_supports_generator(candidate.generator)?;

        log::info!("Selected {} with CMake generator {}", candidate.name, candidate.generator);

        return Ok(CmakePreset {
            visual_studio_name: candidate.name,
            configure: candidate.configure,
            build_debug: candidate.build_debug,
            build_release: candidate.build_release,
        });
    }

    Err("未检测到带 MSVC C++ 工具的 VS2026 或 VS2022，请安装对应的 C++ workload。".to_string())
}

fn find_vswhere() -> Option<PathBuf> {
    let mut candidates = Vec::new();

    if let Some(program_files_x86) = std::env::var_os("ProgramFiles(x86)") {
        candidates.push(PathBuf::from(program_files_x86).join("Microsoft Visual Studio\\Installer\\vswhere.exe"));
    }

    if let Some(program_files) = std::env::var_os("ProgramFiles") {
        candidates.push(PathBuf::from(program_files).join("Microsoft Visual Studio\\Installer\\vswhere.exe"));
    }

    candidates.push(PathBuf::from(r"C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe"));

    candidates.into_iter().find(|path| path.is_file())
}

fn has_msvc_tools(vswhere: &Path, candidate: &VisualStudioCandidate) -> Result<bool, String> {
    let output = Command::new(vswhere)
        .args([
            "-latest",
            "-version",
            candidate.version_range,
            "-products",
            "*",
            "-requires",
            MSVC_TOOLS_COMPONENT,
            "-property",
            "installationPath",
        ])
        .output()
        .map_err(|err| format!("无法执行 {}: {err}", vswhere.display()))?;

    if !output.status.success() {
        return Err(command_error("vswhere", &output));
    }

    let installation_path = String::from_utf8_lossy(&output.stdout);
    Ok(!installation_path.trim().is_empty())
}

fn ensure_cmake_supports_generator(generator: &str) -> Result<(), String> {
    let output =
        Command::new("cmake").arg("--help").output().map_err(|err| format!("无法执行 PATH 上的 cmake: {err}"))?;

    if !output.status.success() {
        return Err(command_error("cmake --help", &output));
    }

    // 只检查 PATH 上的 CMake，避免隐式切到 VS 自带 CMake。
    let help_text = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));

    if help_text.contains(generator) {
        return Ok(());
    }

    Err(format!("PATH 上的 cmake 不支持 {generator}。VS2026 需要 CMake 4.2+，请确认 PATH 指向新版 CMake。"))
}

fn command_error(command: &str, output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    format!(
        "{command} 执行失败，退出码: {}，stdout: {}，stderr: {}",
        output.status.code().map_or_else(|| "unknown".to_string(), |code| code.to_string()),
        stdout.trim(),
        stderr.trim()
    )
}
