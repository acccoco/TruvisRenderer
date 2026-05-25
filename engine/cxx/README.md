# CXX

`engine/cxx/` 提供 C++ 子系统与 Rust FFI 桥接，当前 Rust 侧只暴露 Assimp 绑定。

## 目录说明

- `mods/`：C++ 模块源码；模块之间使用 C++ API，导出到 Rust 时由具体模块提供 C API
- `truvis-assimp-binding/`：Assimp Rust FFI 声明 crate
- `truvis-cxx-build/`：构建驱动 crate，负责选择 CMake preset、构建 Debug/Release，并把 `.lib` / `.dll` / `.pdb` 复制到 Cargo target 目录
- `CMakeLists.txt` / `CMakePresets.json`：CMake 构建配置
- `vcpkg.json`：manifest 依赖声明

## 构建方式

- 推荐通过 workspace 命令执行：`just cxx`
- `just cxx` 会先运行 `cargo run --bin cxx-build`，再构建 `truvis-assimp-binding`
- 底层使用 CMake + vcpkg manifest，不建议手工 `vcpkg install`
- `truvis-assimp-binding/build.rs` 只负责 bindgen 生成 Assimp Rust FFI 绑定，并向 Cargo 声明链接 `truvixx-assimp-capi`

## 约束

- 对外接口保持 C ABI 与 POD 数据结构稳定。
- 不再维护统一 C++ interface target；需要导出到 Rust 的 C API 放在对应 C++ 模块内。
- 变更 FFI 结构时需同步检查 Rust 侧绑定与内存布局兼容性。
- Assimp scene 加载失败时，`truvixx_scene_load` 可能返回可查询错误的非空句柄；调用方必须通过
  `truvixx_scene_is_loaded` 判断成功状态，并通过 `truvixx_scene_last_error` 读取详细失败原因。
