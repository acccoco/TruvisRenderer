set shell := ["powershell.exe", "-c"]

validation_layer_settings := justfile_directory() + "\\tools\\vulkan\\khronos_validation_settings.txt"

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

# 编译 shader 并更新 Rust 绑定
[group('2 资源生成与构建')]
shader:
    cargo run --bin shader-build
    cargo build -p truvis-shader-binding

# 编译 CXX 项目并更新 Rust 绑定
[group('2 资源生成与构建')]
cxx:
    cargo run --bin cxx-build
    cargo build -p truvis-cxx-binding

# 运行 Triangle 示例
[group('3 运行示例')]
triangle: shader (_run-sample "triangle")

# 运行 ShaderToy 示例
[group('3 运行示例')]
shader-toy: shader (_run-sample "shader-toy")

# 运行 Cornell 光追示例
[group('3 运行示例')]
cornell: shader cxx (_run-rt-sample "rt-cornell")

# 使用 Vulkan validation layer 运行 Cornell 光追示例
[group('3 运行示例')]
cornell-validation: shader cxx (_run-rt-validation "rt-cornell")

# 运行 Sponza 主体应用
[group('3 运行示例')]
sponza: shader cxx (_run-rt-sample "rt-sponza")

# 使用 Vulkan validation layer 运行 Sponza 主体应用
[group('3 运行示例')]
sponza-validation: shader cxx (_run-rt-validation "rt-sponza")

# 配置 clang-cl Debug preset
[group('4 CXX CMake 手工入口')]
cxx-preset-clang: (_cxx-preset "clang-cl-debug")

# 配置 clang-cl Release preset
[group('4 CXX CMake 手工入口')]
cxx-preset-clang-release: (_cxx-preset "clang-cl-release")

# 配置 Visual Studio 2022 preset
[group('4 CXX CMake 手工入口')]
cxx-preset-vs2022: (_cxx-preset "vs2022")

# 配置 Visual Studio 2022 preset 的兼容别名
[group('4 CXX CMake 手工入口')]
cxx-preset-vs: (_cxx-preset "vs2022")

# 配置 Visual Studio 2026 preset
[group('4 CXX CMake 手工入口')]
cxx-preset-vs2026: (_cxx-preset "vs2026")

# 构建 Visual Studio 2022 Debug preset
[group('4 CXX CMake 手工入口')]
cxx-build-vs2022: (_cxx-build "vs2022-build-debug")

# 构建 Visual Studio 2022 Debug preset 的兼容别名
[group('4 CXX CMake 手工入口')]
cxx-build-vs: (_cxx-build "vs2022-build-debug")

# 构建 Visual Studio 2026 Debug preset
[group('4 CXX CMake 手工入口')]
cxx-build-vs2026: (_cxx-build "vs2026-build-debug")

# 构建 clang-cl Debug preset
[group('4 CXX CMake 手工入口')]
cxx-build-clang: (_cxx-build "clang-cl-build-debug")

# 构建 clang-cl Release preset
[group('4 CXX CMake 手工入口')]
cxx-build-clang-release: (_cxx-build "clang-cl-build-release")

_run-sample bin:
    cargo run --bin {{ bin }}

_run-rt-sample bin:
    cargo run --bin {{ bin }}

_run-rt-validation bin:
    $env:VK_LOADER_LAYERS_ENABLE='VK_LAYER_KHRONOS_validation'; $env:VK_LAYER_SETTINGS_PATH='{{ validation_layer_settings }}'; cargo run --bin {{ bin }}

[working-directory("engine/cxx")]
_cxx-preset preset:
    cmake --preset {{ preset }}

[working-directory("engine/cxx")]
_cxx-build preset:
    cmake --build --preset {{ preset }}
