# Engine

`engine/` 是渲染引擎核心实现目录，包含 Rust 分层模块、Shader 工具链与 C++ FFI 子系统。

## 子目录

- `foundation/`：基础工具 crate，如日志与通用数据结构。
- `utils/`：引擎内部工具 crate，如路径管理与资源拉取。
- `gfx/`：Vulkan RHI 封装与 descriptor-layout 宏/trait。
- `world/`：CPU 侧资产、场景与 `World` 聚合。
- `render/`：渲染资源状态、RenderGraph、通用 pass、GUI 后端与 RenderBackend。
- `frame/`：`RenderApp` 契约与 `RenderAppShell` 帧骨架。
- `app/`：示例应用、plugin 编排与 render pipeline glue。
- `shader/`：Slang shader 源码、共享头文件、编译与绑定生成。
- `cxx/`：Assimp 场景加载、CMake 工程、Rust 绑定桥接。

## 推荐阅读顺序

1. `../ARCHITECTURE.md`：了解总体分层、依赖方向与生命周期约束。
2. 各分层目录和 crate 内 README：理解具体模块职责。
3. `shader/README.md`、`cxx/README.md`：了解工具链与外部边界。
