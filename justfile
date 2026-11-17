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

# 拉取资源与工具，可追加 --verbose
[group('2 资源生成与构建')]
fetch-res *resource_names:
    #!nu
    # just 无参数时会插入一个空字符串；过滤后再构造 cargo 参数，避免空字符串被当成资源名。
    let resource_names = '{{ resource_names }}' | split row ' ' | where {|name| $name != '' }
    let args = ['run', '--bin', 'fetch_res', '--'] ++ $resource_names
    cargo ...$args

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

# 增量编译 shader 并更新 Rust 绑定，可追加 --verbose
[group('2 资源生成与构建')]
shader *tool_opts:
    cargo run --bin shader-build -- --manifest shader-packages.toml {{ tool_opts }}
    cargo build -p truvis-shader-binding -p truvis-renderer-shader-binding

# 强制重新编译全部 shader 并更新 Rust 绑定，可追加 --verbose
[group('2 资源生成与构建')]
shader-force *tool_opts:
    cargo run --bin shader-build -- --manifest shader-packages.toml --force {{ tool_opts }}
    cargo build -p truvis-shader-binding -p truvis-renderer-shader-binding

# 构建 Debug native 与三个 Rust binding，并由 binding 复制运行文件
[group('2 资源生成与构建')]
cxx-debug:
    nu scripts/nu/cxx-cmake.nu '{{ justfile_directory() }}' debug

# 只生成 clangd 数据库，不构建 native target 或 Rust binding
[group('2 资源生成与构建')]
cxx-compile-commands:
    nu scripts/nu/cxx-cmake.nu '{{ justfile_directory() }}' --compile-commands

# 顺序构建 Debug、Release native 与 Rust binding
[group('2 资源生成与构建')]
cxx:
    nu scripts/nu/cxx-cmake.nu '{{ justfile_directory() }}' all

# CMake clean-first 后重新构建两种配置
[group('2 资源生成与构建')]
cxx-force:
    nu scripts/nu/cxx-cmake.nu '{{ justfile_directory() }}' all --force

# 运行 Triangle 示例
[group('3 运行示例')]
triangle *run_opts: shader cxx-debug (_run-cargo-bin "triangle" run_opts)

# 运行 ShaderToy 示例
[group('3 运行示例')]
shader-toy *run_opts: shader cxx-debug (_run-cargo-bin "shader-toy" run_opts)

# 运行完整 TruvisRenderer 的 standalone 入口，默认 Manual；Cornell Box 使用 --scene cornell
[group('3 运行示例')]
cornell *run_opts: shader cxx-debug (_run-cargo-bin "rt-cornell" run_opts)

# 构建 Tauri WebView 前端后运行 Truvis 主体应用；可追加 imgui / no-validation / --scene 选项
[group('3 运行示例')]
truvis *run_opts: editor-web shader cxx-debug (_run-cargo-bin "truvis-app" run_opts)

# 构建 Tauri WebView 前端后直接运行 Truvis 主体应用，不更新 shader / CXX 绑定；可追加 imgui / no-validation / --scene 选项
[group('3 运行示例')]
truvis-direct *run_opts: editor-web (_run-cargo-bin "truvis-app" run_opts)

# 启动 Tracy Profiler
[group('4 工具入口')]
tracy:
    start '{{ tracy_profiler }}'

# Web editor 的 TypeScript 协议必须从 Rust DTO 生成；该内部 recipe 不作为日常命令暴露。
_editor-web-types:
    cargo run -p truvis-editor-bridge --bin export_editor_types

_run-cargo-bin bin *run_opts:
    nu scripts/nu/run-cargo-bin.nu '{{ justfile_directory() }}' '{{ bin }}' {{ run_opts }}
