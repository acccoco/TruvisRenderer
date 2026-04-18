# Engine

`engine/` 是渲染引擎核心实现目录，包含 Rust 模块、Shader 工具链与 C++ FFI 子系统。

## 子目录

- `crates/`：Rust 核心 crate，按分层组织渲染框架
- `shader/`：Slang shader 源码、共享头文件、编译与绑定生成
- `cxx/`：Assimp 场景加载、CMake 工程、Rust 绑定桥接

## 推荐阅读顺序

1. `../ARCHITECTURE.md`：先了解总体分层与边界
2. `crates/README.md`：理解 Rust 主干模块职责
3. `shader/README.md`、`cxx/README.md`：了解工具链与外部边界
