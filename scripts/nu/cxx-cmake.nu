def main [repo_root: path, action: string, tool: string, profile: string] {
    let action = ($action | str downcase)
    let tool = ($tool | str downcase)
    let profile = ($profile | str downcase)
    if ($action not-in ['preset', 'build']) {
        error make { msg: $"Unsupported CXX action '($action)'. Use 'preset' or 'build'." }
    }
    if ($profile not-in ['debug', 'release']) {
        error make { msg: $"Unsupported CXX profile '($profile)'. Use 'debug' or 'release'." }
    }

    let configure_preset = match $tool {
        'vs2022' => 'vs2022'
        'vs2026' => 'vs2026'
        'clang' => {
            if $profile == 'debug' {
                'clang-cl-debug'
            } else {
                'clang-cl-release'
            }
        }
        _ => {
            error make { msg: $"Unsupported CXX tool '($tool)'. Use 'vs2022', 'vs2026', or 'clang'." }
        }
    }
    let build_preset = match $tool {
        'vs2022' => $"vs2022-build-($profile)"
        'vs2026' => $"vs2026-build-($profile)"
        'clang' => $"clang-cl-build-($profile)"
        _ => {
            error make { msg: $"Unsupported CXX tool '($tool)'. Use 'vs2022', 'vs2026', or 'clang'." }
        }
    }

    cd ($repo_root | path join 'cxx')
    if $action == 'preset' {
        cmake --preset $configure_preset
    } else {
        cmake --build --preset $build_preset
    }
}
