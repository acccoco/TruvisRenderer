set shell := ["nu", "-c"]

tracy_profiler := justfile_directory() + "\\external\\tracy\\tracy-profiler.exe"

# 显示可用命令
[group('1 常用工作流')]
default:
    @just --list

# 构建 Web editor、shader、CXX 绑定与整个 workspace
[group('1 常用工作流')]
build-all: editor-web shader cxx
    cargo build --all

# 拉取资源与工具
[group('2 资源生成与构建')]
fetch-res:
    cargo run --bin fetch_res

# 将一个 Blender/FBX 场景转换为 glTF 和外部贴图（需要 Python/Pillow 和 Blender）
[group('2 资源生成与构建')]
scene-export source output blender="C:/Program Files/Blender Foundation/Blender 5.2/blender.exe":
    python scripts/scene/export_gltf.py --source '{{ source }}' --output-dir '{{ output }}' --blender '{{ blender }}'

# 生成协议类型并构建 Web editor 生产资源
[group('2 资源生成与构建')]
[working-directory("app/editor/web")]
editor-web: _editor-web-types
    npm install
    npm run build

# 生成协议类型并启动 Web editor 开发服务器
[group('2 资源生成与构建')]
[working-directory("app/editor/web")]
editor-web-dev: _editor-web-types
    npm install
    npm run dev

# 增量编译 shader 并更新 Rust 绑定
[group('2 资源生成与构建')]
shader:
    cargo run --bin shader-build -- --manifest shader-packages.toml
    cargo build -p truvis-shader-binding -p truvis-renderer-shader-binding

# 强制重新编译全部 shader 并更新 Rust 绑定
[group('2 资源生成与构建')]
shader-force:
    cargo run --bin shader-build -- --manifest shader-packages.toml --force
    cargo build -p truvis-shader-binding -p truvis-renderer-shader-binding

# 增量准备 Debug CXX 产物，供 dev cargo run / just truvis 使用
[group('2 资源生成与构建')]
cxx-debug:
    cargo run --bin cxx-build -- --profile debug
    just _cxx-bindings

# 增量编译 Debug + Release CXX 项目并更新 Rust 绑定
[group('2 资源生成与构建')]
cxx:
    cargo run --bin cxx-build -- --profile all
    just _cxx-bindings

# 强制重新编译 Debug + Release CXX 项目并更新 Rust 绑定
[group('2 资源生成与构建')]
cxx-force:
    cargo run --bin cxx-build -- --profile all --force
    just _cxx-bindings

# 从 Cargo package metadata 发现所有单向消费 public CXX DLL 的 binding crate。
_cxx-bindings:
    nu scripts/nu/build-cxx-bindings.nu '{{ justfile_directory() }}'

# 运行 Triangle 示例
[group('3 运行示例')]
triangle *run_opts: shader (_run-cargo-bin "triangle" run_opts)

# 运行 ShaderToy 示例
[group('3 运行示例')]
shader-toy *run_opts: shader (_run-cargo-bin "shader-toy" run_opts)

# 运行 Cornell 光追示例
[group('3 运行示例')]
cornell *run_opts: shader cxx-debug (_run-cargo-bin "rt-cornell" run_opts)

# 构建 Tauri WebView 前端后运行 Truvis 主体应用；可追加 imgui / no-validation 选项
[group('3 运行示例')]
truvis *run_opts: editor-web shader cxx-debug (_run-cargo-bin "truvis-app" run_opts)

# 构建 Tauri WebView 前端后直接运行 Truvis 主体应用，不更新 shader / CXX 绑定；可追加 imgui / no-validation 选项
[group('3 运行示例')]
truvis-direct *run_opts: editor-web (_run-cargo-bin "truvis-app" run_opts)

# 启动 Tracy Profiler
[group('4 工具入口')]
tracy:
    start '{{ tracy_profiler }}'

# 配置 cxx/ CMake preset：tool=vs2026/vs2022/clang，profile=debug/release
[group('5 CXX CMake 手工入口')]
cxx-preset tool="vs2026" profile="debug": (_cxx-cmake "preset" tool profile)

# 构建 cxx/ CMake preset：tool=vs2026/vs2022/clang，profile=debug/release
[group('5 CXX CMake 手工入口')]
cxx-build tool="vs2026" profile="debug": (_cxx-cmake "build" tool profile)

# Web editor 的 TypeScript 协议必须从 Rust DTO 生成；该内部 recipe 不作为日常命令暴露。
_editor-web-types:
    cargo run -p truvis-editor-bridge --bin export_editor_types

_run-cargo-bin bin *run_opts:
    nu scripts/nu/run-cargo-bin.nu '{{ justfile_directory() }}' '{{ bin }}' {{ run_opts }}

_cxx-cmake action tool profile:
    nu scripts/nu/cxx-cmake.nu '{{ justfile_directory() }}' '{{ action }}' '{{ tool }}' '{{ profile }}'
