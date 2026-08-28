# CXX

`engine/cxx/` 提供 C++ 子系统、CMake/vcpkg 构建和 Rust FFI 桥接，当前对 Rust 暴露 Assimp 与 Streamline 能力。

## 目录职责

- `mods/`：C++ 模块源码；模块之间使用 C++ API，需要导出到 Rust 的能力由具体模块提供 C API。
- `mods/truvixx-utils/`：Windows 路径、字符串编码和文件系统公共工具。
- `truvis-assimp-binding/`：Assimp C API 的 Rust FFI 声明与链接边界。
- `truvis-streamline-binding/`：Streamline/DLSS Rust FFI 与最小 RAII runtime。
- `truvis-cxx-build/`：选择 CMake preset、构建 native 产物、生成绑定并复制运行时文件的 workspace 工具。
- `CMakeLists.txt` / `CMakePresets.json` / `vcpkg.json`：native 构建与依赖配置。

详细命令、输出目录、环境要求和手工 preset 见 [`build.md`](build.md)。

## 绑定与产物边界

- Assimp 与 Streamline binding 的 `build.rs` 只负责 bindgen、link search 和 link library 声明，
  不负责调用 CMake 或复制 native runtime。
- Rust FFI binding 生成到 `build/bindings/{TARGET}/cxx/{crate}/`，源码树不保存 `_ffi_bindings.rs`。
- `truvis-cxx-build` 按 profile 把 native `.lib`/`.dll`/`.pdb`、Streamline runtime DLL 和项目维护的 JSON
  复制到 Cargo executable 目录。
- Streamline C++ wrapper 不链接 `sl.interposer.lib`；Rust 传入 `sl.interposer.dll` 绝对路径，C++ 通过
  `LoadLibraryW`/`GetProcAddress` 解析生命周期入口。

## 约束

- 对外接口保持 C ABI 与 POD 数据结构；修改 FFI 结构时必须同步检查 Rust binding、字段宽度和内存布局。
- 不维护统一 C++ interface target；C API 留在拥有对应生命周期与业务语义的 C++ 模块内。
- C++ 模块重复的路径、UTF-16/UTF-8 转换和目录创建逻辑归 `truvixx-utils`，业务模块不复制 helper。
- Streamline C API 负责 `slInit`/`slShutdown`、feature query/options、resource tagging/evaluate 与 resource free；
  RenderGraph pass 顺序和 Vulkan 资源生命周期仍由 Rust Renderer 层负责。
- Streamline callback 只复制消息并入队，最终日志输出由 Rust `streamline-logger` 线程完成。
- Assimp scene 加载失败时 C API 可能返回可查询错误的非空句柄；调用方必须先检查 loaded 状态，再读取错误并释放句柄。
- Streamline 接入当前只面向 Windows x64，不保留没有真实实现的跨平台 cfg 分支。

Streamline 日志、队列和生命周期细节见 [`truvis-streamline-binding/README.md`](truvis-streamline-binding/README.md)。
