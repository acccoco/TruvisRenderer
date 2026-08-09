# CXX 构建与排查

本文件是 `engine/cxx/` 唯一的构建操作入口。模块职责、FFI owner 和生命周期约束见 [`README.md`](README.md)。

## Workspace 命令

日常运行 Truvis 只需要 Debug CXX 产物：

```powershell
just cxx-debug
```

同时准备 Debug 与 Release：

```powershell
just cxx
```

绕过增量 manifest、强制重新生成全部 CXX 产物与绑定：

```powershell
just cxx-force
```

`just cxx-debug` 和 `just cxx` 通过 `truvis-cxx-build` 选择 CMake preset、构建 native target、
更新 Rust binding，并把运行时 `.lib`/`.dll`/`.pdb` 与 Streamline JSON 复制到 Cargo 输出目录。

## 目录与增量状态

- CMake binary dir：`build/cxx/{vs2022,vs2026,clang-cl}`。
- Native 输出：`build/cxx/output/{Debug,Release}`。
- Cargo executable/runtime 目录：`build/{debug,release}` 及其 `examples/`。
- CXX 增量状态：`build/cxx/.state/`。
- Rust FFI binding：`build/bindings/{TARGET}/cxx/{crate}/`。
- Clangd 数据库：`build/cxx/compile_commands.json` 和 `.vscode/compile_commands.json`。

输入、preset、关键环境和预期输出未变化时，`truvis-cxx-build` 会跳过 CMake configure/build，
只检查运行目录复制。需要验证真实 clean build path 时使用 `just cxx-force`。

## 环境要求

- PATH 上可用的 CMake；CMake presets 需要 3.21+，VS2026 preset 需要 4.2+。
- Visual Studio 2022 或 2026，并安装 MSVC C++ workload。
- 已设置 `VCPKG_ROOT`，指向包含 `scripts/buildsystems/vcpkg.cmake` 的 vcpkg checkout。
- 使用 clang-cl preset 时，PATH 上需要 LLVM/clang-cl 与 Ninja。

不要手工执行 `vcpkg install`；项目使用 `vcpkg.json` manifest。更新本地 vcpkg checkout 后，如确需推进项目 baseline，
在 workspace 根目录显式运行 `vcpkg x-update-baseline` 并把 `vcpkg.json` 变化作为独立依赖更新审查。

## 手工 Preset 入口

正常开发优先使用上述 workspace 命令。排查 CMake 问题时可以直接运行已有 preset：

```powershell
cmake --preset vs2026
cmake --build --preset vs2026-build-debug

cmake --preset vs2022
cmake --build --preset vs2022-build-debug

cmake --preset clang-cl-debug
cmake --build --preset clang-cl-build-debug
```

也可以使用 `just cxx-preset vs2022 debug` 或 `just cxx-build clang release` 统一调用。
第一个参数支持 `vs2026`、`vs2022`、`clang`，第二个参数支持 `debug`、`release`。

`compile_commands.json` 由 `truvis-cxx-build` 尝试通过 `clang-cl-debug` preset 生成。clang-cl 或 Ninja 不可用时，
只跳过 IDE 数据库同步，不阻断 Visual Studio preset 构建。
