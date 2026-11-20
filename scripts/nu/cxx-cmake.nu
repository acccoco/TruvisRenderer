# CMake 统一先构建，Cargo binding 随后消费；任一步失败立即停止。
def main [repo_root: path, profile: string = "debug", --force, --compile-commands] {
    cd ($repo_root | path join 'cxx')
    if $compile_commands {
        cmake --preset clang-cl-debug
        if $env.LAST_EXIT_CODE != 0 { exit $env.LAST_EXIT_CODE }
        let source = ($repo_root | path join 'build/cxx/cmake/clang-cl/Debug/compile_commands.json')
        mkdir ($repo_root | path join '.vscode')
        cp $source ($repo_root | path join 'build/cxx/compile_commands.json')
        cp $source ($repo_root | path join '.vscode/compile_commands.json')
        return
    }

    let tool = ($env.TRUVIS_CXX_TOOL? | default 'clang-cl')
    if ($tool not-in ['vs2022', 'vs2026', 'clang-cl']) {
        error make { msg: $"Unsupported TRUVIS_CXX_TOOL: ($tool)" }
    }
    if ($profile not-in ['debug', 'release', 'all']) {
        error make { msg: $"Unsupported CXX profile: ($profile)" }
    }
    let profiles = if $profile == 'all' { ['debug', 'release'] } else { [$profile] }
    let clean_args = if $force { ['--clean-first'] } else { [] }
    for current in $profiles {
        cd ($repo_root | path join 'cxx')
        let configure = if $tool == 'clang-cl' { $"clang-cl-($current)" } else { $tool }
        cmake --preset $configure
        if $env.LAST_EXIT_CODE != 0 { exit $env.LAST_EXIT_CODE }
        cmake --build --preset $"($tool)-build-($current)" ...$clean_args
        if $env.LAST_EXIT_CODE != 0 { exit $env.LAST_EXIT_CODE }

        cd $repo_root
        let release_args = if $current == 'release' { ['--release'] } else { [] }
        cargo build -p truvis-assimp-binding -p truvis-streamline-binding -p truvis-vk-binding ...$release_args
        if $env.LAST_EXIT_CODE != 0 { exit $env.LAST_EXIT_CODE }
    }
}
