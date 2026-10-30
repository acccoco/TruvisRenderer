# CMake Helpers

`TruvisCxx.cmake` 只提供两类 workspace-specific 能力：

- 为 public SHARED C API target 生成统一位置的 export header，并在 configure 时验证 target 类型。
- 收集当前 public target 输出与 module 声明的 required/optional runtime 文件；在 build 后生成 Debug/Release 完整 runtime plan。

`TruvisRuntimePlan.cmake` 是生成脚本的构建期执行体：从 target 声明的 DLL 来源递归解析实际依赖，排除 Windows
系统目录，再发布完整清单。它补足 imported target 未暴露的间接 DLL，不扫描输出目录，也不替 Rust 执行部署。
当前 Windows 工具链使用 `dumpbin`（由 CMake 定位）；依赖无法解析时 build 失败，runner 不进入部署。

Module sources、include、第三方包和 target 依赖继续使用标准 CMake
`add_library`、`target_include_directories` 与 `target_link_libraries`。这里不维护 Engine/Renderer/App owner、
自定义 module graph、Cargo package 或 Rust 类型信息。
