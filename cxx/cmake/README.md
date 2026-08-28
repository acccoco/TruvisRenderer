# CMake Helpers

`TruvisCxx.cmake` 只提供两类 workspace-specific 能力：

- 为 public SHARED C API target 生成统一位置的 export header，并在 configure 时验证 target 类型。
- 收集 module 声明的 required/optional runtime 文件，生成 Debug/Release runtime plan。

Module sources、include、第三方包和 target 依赖继续使用标准 CMake
`add_library`、`target_include_directories` 与 `target_link_libraries`。这里不维护 Engine/Renderer/App owner、
自定义 module graph、Cargo package 或 Rust 类型信息。
