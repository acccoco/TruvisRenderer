# CMake Helpers

`TruvisCxx.cmake` 为 public SHARED C API target 生成统一位置的 export header，并验证 target 类型。
Module sources、include、第三方包和 target 依赖使用标准 CMake `add_library`、`target_include_directories`
与 `target_link_libraries`。运行文件由各 Rust binding 显式声明；CMake 不生成部署清单，不调用 Cargo。
