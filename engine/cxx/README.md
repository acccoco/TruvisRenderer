# CXX

`engine/cxx/` 提供 C++ 子系统与 Rust FFI 桥接，当前 Rust 侧暴露 Assimp 与 Streamline 绑定。

## 目录说明

- `mods/`：C++ 模块源码；模块之间使用 C++ API，导出到 Rust 时由具体模块提供 C API
- `mods/truvixx-utils/`：C++ 公共工具 static library，通过 `PathUtils` / `StringUtils` 聚合 Windows 路径、字符串编码和文件系统 helper
- `truvis-assimp-binding/`：Assimp Rust FFI 声明 crate
- `truvis-streamline-binding/`：Streamline / DLSS Rust FFI 声明与最小 RAII wrapper
- `truvis-cxx-build/`：构建驱动 crate，负责选择 CMake preset、按 profile 增量构建 CXX，把 `.lib` / `.dll` / `.pdb` 复制到 Cargo 输出目录（当前为 `build/{profile}`），并同步 `compile_commands.json`
- `CMakeLists.txt` / `CMakePresets.json`：CMake 构建配置
- `vcpkg.json`：manifest 依赖声明

## 构建方式

- 日常运行 `just truvis` 时会通过 `just cxx-debug` 只准备 Debug CXX 产物，避免 dev `cargo run` 前无意义构建 Release。
- 需要完整刷新 Debug + Release 时执行 `just cxx`；需要绕过 manifest 时执行 `just cxx-force`。
- `just cxx-debug` / `just cxx` 会先运行 `cargo run --bin cxx-build -- --profile ...`，再构建 `truvis-assimp-binding` 与 `truvis-streamline-binding`
- 底层使用 CMake + vcpkg manifest，不建议手工 `vcpkg install`
- `truvis-assimp-binding/build.rs` 只负责 bindgen 生成 Assimp Rust FFI 绑定，并向 Cargo 声明链接 `truvixx-assimp-capi`
- `truvis-streamline-binding/build.rs` 只负责 bindgen 生成 Streamline C API 绑定，并向 Cargo 声明链接 `truvixx-streamline-capi`
- `truvis-cxx-build` 会按 profile 复制 Streamline SR/RR runtime DLL：Debug 使用 `tools/streamline-sdk/bin/x64/development`，Release 使用 `tools/streamline-sdk/bin/x64`；运行时 JSON 从项目维护的 `tools/streamline/` 复制。
- CMake binary dir 和 native 输出目录位于 workspace 根目录的 `build/cxx/`：preset 中间产物位于 `build/cxx/{vs2022,vs2026,clang-cl}`，`.lib` / `.dll` / `.pdb` 输出位于 `build/cxx/output/{Debug,Release}`。
- 当前 Cargo 输出目录由 `.cargo/config.toml` 指向 `build/`；native runtime DLL 和 Streamline JSON 会被复制到 `build/{profile}` 和 `build/{profile}/examples`，与最终 executable 同目录。
- `truvis-cxx-build` 在 `build/cxx/.state/` 维护 profile 级 manifest；CXX 输入、CMake preset 和关键环境未变化且输出仍存在时，会跳过 CMake configure/build，只做必要的运行目录复制检查。
- `compile_commands.json` 在需要重新执行 CMake 构建时由 `truvis-cxx-build` 通过 `clang-cl-debug` preset 生成，并同步到 `build/cxx/compile_commands.json` 和 `.vscode/compile_commands.json`；如果 clang-cl 或 Ninja 不可用，只跳过同步，不阻断 Visual Studio 构建。
- Streamline 接入当前只面向 Windows x64，Rust binding 直接使用 Windows 路径编码和 DLL 加载约定，不保留跨平台 cfg 分支
- Streamline C++ wrapper 不链接 `sl.interposer.lib`；Rust 侧把 `sl.interposer.dll` 绝对路径传入 C API，C++ 再通过 `LoadLibraryW` / `GetProcAddress` 显式解析 `slInit` / `slShutdown`。
- Streamline 日志由 C++ wrapper 接住 `logMessageCallback` 后转发给 Rust；详细链路见 `truvis-streamline-binding/README.md`

## 约束

- 对外接口保持 C ABI 与 POD 数据结构稳定。
- 不再维护统一 C++ interface target；需要导出到 Rust 的 C API 放在对应 C++ 模块内。
- 变更 FFI 结构时需同步检查 Rust 侧绑定与内存布局兼容性。
- C++ 模块内重复的路径、UTF-16 / UTF-8 转换和目录创建逻辑优先放入 `truvixx-utils` 的静态工具 struct，业务模块只保留自身生命周期和 API 语义。
- Streamline C API 覆盖 `slInit/slShutdown` 生命周期、DLSS SR/RR support query、options、resource tagging、evaluate 与 resource free；RenderGraph pass 顺序和 Vulkan 资源生命周期仍由 Rust/app 层负责。
- Streamline callback 可能来自 init/shutdown 或 Vulkan interposer 调用栈；Rust callback 内只做消息复制和入队，最终日志输出在 `streamline-logger` 线程完成。
- Assimp scene 加载失败时，`truvixx_scene_load` 可能返回可查询错误的非空句柄；调用方必须通过
  `truvixx_scene_is_loaded` 判断成功状态，并通过 `truvixx_scene_last_error` 读取详细失败原因。
