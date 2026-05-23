# Engine

`engine/` 是渲染引擎核心实现目录，包含 Rust 分层模块、Shader 工具链与 C++ FFI 子系统。

## 子目录

- `foundation/`：基础工具 crate，如日志与通用数据结构。
- `utils/`：引擎内部工具 crate，如路径管理与资源拉取。
- `gfx/`：Vulkan RHI 封装与 descriptor-layout 宏/trait。
- `world/`：CPU 侧资产、场景与 `World` 聚合。
- `render/`：渲染资源状态、RenderGraph、通用 pass、GUI 后端与 RenderRuntime。
- `frame/`：`RenderApp` 契约、`RenderAppShell` 帧骨架与渲染线程主循环。
- `shader/`：Slang shader 源码、共享头文件、编译与绑定生成。
- `cxx/`：Assimp 场景加载、CMake 工程、Rust 绑定桥接。

应用层位于 workspace 顶层 `truvis-app/`，其中 `app-kit/` 保存 GUI、输入/相机、overlay
和 RT pipeline glue，`sponza/` 与 `samples/` 保存具体可执行入口。

## 推荐阅读顺序

1. `../ARCHITECTURE.md`：了解总体分层、依赖方向与生命周期约束。
2. 各分层目录和 crate 内 README：理解具体模块职责。
3. `shader/README.md`、`cxx/README.md`：了解工具链与外部边界。
