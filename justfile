set shell := ["nu", "-c"]

validation_layer_settings := justfile_directory() + "\\tools\\vulkan\\khronos_validation_settings.txt"
tracy_profiler := justfile_directory() + "\\tools\\tracy\\tracy-profiler.exe"

# 显示可用命令
[group('1 常用工作流')]
default:
    @just --list

# 构建 shader、CXX 绑定与整个 workspace
[group('1 常用工作流')]
build-all: shader cxx
    cargo build --all

# 拉取资源与工具
[group('2 资源生成与构建')]
fetch-res:
    cargo run --bin fetch_res

# 增量编译 shader 并更新 Rust 绑定
[group('2 资源生成与构建')]
shader:
    cargo run --bin shader-build
    cargo build -p truvis-shader-binding

# 强制重新编译全部 shader 并更新 Rust 绑定
[group('2 资源生成与构建')]
shader-force:
    cargo run --bin shader-build -- --force
    cargo build -p truvis-shader-binding

# 增量准备 Debug CXX 产物，供 dev cargo run / just truvis 使用
[group('2 资源生成与构建')]
cxx-debug:
    cargo run --bin cxx-build -- --profile debug
    cargo build -p truvis-assimp-binding -p truvis-streamline-binding

# 增量编译 Debug + Release CXX 项目并更新 Rust 绑定
[group('2 资源生成与构建')]
cxx:
    cargo run --bin cxx-build -- --profile all
    cargo build -p truvis-assimp-binding -p truvis-streamline-binding

# 强制重新编译 Debug + Release CXX 项目并更新 Rust 绑定
[group('2 资源生成与构建')]
cxx-force:
    cargo run --bin cxx-build -- --profile all --force
    cargo build -p truvis-assimp-binding -p truvis-streamline-binding

# 运行 Triangle 示例
[group('3 运行示例')]
triangle *run_opts: shader (_run-cargo-bin "triangle" run_opts)

# 运行 ShaderToy 示例
[group('3 运行示例')]
shader-toy *run_opts: shader (_run-cargo-bin "shader-toy" run_opts)

# 运行 Cornell 光追示例
[group('3 运行示例')]
cornell *run_opts: shader cxx-debug (_run-cargo-bin "rt-cornell" run_opts)

# 运行 Truvis 主体应用；可追加 imgui / no-validation 选项
[group('3 运行示例')]
truvis *run_opts: shader cxx-debug (_run-cargo-bin "truvis-app" run_opts)

# 直接运行 Truvis 主体应用，不更新 shader / CXX 绑定；可追加 imgui / no-validation 选项
[group('3 运行示例')]
truvis-direct *run_opts: (_run-cargo-bin "truvis-app" run_opts)

# 启动 Tracy Profiler
[group('4 工具入口')]
tracy:
    start '{{ tracy_profiler }}'

# 配置 CXX CMake preset：tool=vs2026/vs2022/clang，profile=debug/release
[group('5 CXX CMake 手工入口')]
cxx-preset tool="vs2026" profile="debug": (_cxx-cmake "preset" tool profile)

# 构建 CXX CMake preset：tool=vs2026/vs2022/clang，profile=debug/release
[group('5 CXX CMake 手工入口')]
cxx-build tool="vs2026" profile="debug": (_cxx-cmake "build" tool profile)

_run-cargo-bin bin *run_opts:
    #!nu
    # 所有示例的 cargo run --bin 都统一经过这里，避免 sample / rt sample / Truvis 分散维护启动环境。
    # run_opts 采用宽松开关语义：只识别当前需要的选项，其它参数不做额外校验，保持 justfile 足够轻量。
    let opts = '{{ run_opts }}' | split row ' ' | compact
    let is_truvis_bin = ('{{ bin }}' == 'truvis-app')

    let enable_imgui = $opts | any {|opt| $opt == 'imgui' }
    let enable_validation = not ($opts | any {|opt| $opt == 'no-validation' })

    # Truvis 通过 Streamline 环境变量控制 SL ImGui：默认关闭，只有显式传入 imgui 时开启。
    # 其它示例即使传入 imgui 也不会读取该环境变量，因此无需额外报错。
    # Rust 侧仍会按 TRUVIS_STREAMLINE_IMGUI 做 Debug/Release 保护；这里负责提供确定的 Truvis 启动环境。
    if $is_truvis_bin {
        $env.TRUVIS_STREAMLINE_IMGUI = if $enable_imgui { '1' } else { '0' }
    }

    # Vulkan validation 默认开启；no-validation 只在需要减少调试开销或规避 layer 问题时使用。
    if $enable_validation {
        $env.VK_LOADER_LAYERS_ENABLE = 'VK_LAYER_KHRONOS_validation'
        $env.VK_LAYER_SETTINGS_PATH = '{{ validation_layer_settings }}'
    }

    cargo run --bin {{ bin }}

[working-directory("engine/cxx")]
_cxx-cmake action tool profile:
    #!nu
    # CXX 手工入口的参数化规则集中在这里，避免 preset/build 两条路径维护两份 tool/profile 映射。
    # action 只由上层 recipe 传入，用户侧暴露的是 cxx-preset / cxx-build 两个更明确的入口。
    let action = '{{ action }}' | str downcase

    # tool/profile 对用户大小写宽容，但后续映射统一使用小写，减少分支组合。
    let tool = '{{ tool }}' | str downcase
    let profile = '{{ profile }}' | str downcase

    # 先做白名单校验，再执行 cmake；这样拼写错误会停在 just 层，不会落到难读的 CMake preset 报错。
    if ($action not-in ['preset', 'build']) {
        error make { msg: $"Unsupported CXX action '($action)'. Use 'preset' or 'build'." }
    }

    if ($profile not-in ['debug', 'release']) {
        error make { msg: $"Unsupported CXX profile '($profile)'. Use 'debug' or 'release'." }
    }

    # VS preset 是 multi-config：configure preset 不区分 Debug/Release，build preset 才选择 configuration。
    # clang-cl preset 是 single-config：configure 和 build preset 都需要区分 Debug/Release。
    # 因此这里同时计算 configure/build 两套 preset，最后按 action 选择实际调用哪一个。
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

    if $action == 'preset' {
        cmake --preset $configure_preset
    } else {
        cmake --build --preset $build_preset
    }
