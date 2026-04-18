# CXX

`engine/cxx/` 提供 C++ 资产加载子系统与 Rust FFI 桥接，当前以 Assimp 为核心。

## 目录说明

- `mods/`：C++ 模块源码（接口层 + Assimp 实现层）
- `truvis-cxx-binding/`：Rust FFI 声明 crate
- `truvis-cxx-build/`：构建驱动 crate
- `CMakeLists.txt` / `CMakePresets.json`：CMake 构建配置
- `vcpkg.json`：manifest 依赖声明

## 构建方式

- 推荐通过 workspace 命令执行：`cargo run --bin cxx-build`
- 底层使用 CMake + vcpkg manifest，不建议手工 `vcpkg install`

## 约束

- 对外接口保持 C ABI 与 POD 数据结构稳定。
- 变更 FFI 结构时需同步检查 Rust 侧绑定与内存布局兼容性。
