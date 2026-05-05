# Engine Crates

`engine/crates/` 存放引擎 Rust 核心 crate。该目录是运行时框架、渲染管线与场景系统的主要实现区域。

## 关键主干

`truvis-gfx` -> `truvis-render-interface` -> `truvis-render-graph` -> `truvis-renderer` -> `truvis-frame-api` / `truvis-frame-runtime` -> `truvis-app`

## 主要模块

- `truvis-gfx`：Vulkan RHI 抽象
- `truvis-render-interface`：渲染契约与资源管理原语
- `truvis-render-graph`：声明式渲染图
- `truvis-renderer`：backend 执行与系统整合（swapchain/cmd/sync/submit/present + GPU 上传）
- `truvis-frame-api`：`FrameApp` / `FrameAppHooks` / `Plugin` 契约与 Plugin typed contexts
- `truvis-frame-runtime`：`BaseApp` 帧骨架（Renderer 生命周期 + 输入事件队列）
- `truvis-render-passes`：通用 render pass 实现（RT / 累积 / 降噪 / 色调映射 / blit / resolve / phong）
- `truvis-app`：示例应用、GUI plugin、overlay plugin 与 render pipeline plugin 编排
- `truvis-scene` / `truvis-asset`：场景与资产系统
- `truvis-gui-backend`：ImGui Vulkan 后端

## 协作建议

- 新功能优先复用现有层次，不跨层直接依赖。
- 涉及多模块改动时先确认依赖方向是否仍是 DAG。
- 具体边界约束见 `../../ARCHITECTURE.md`。
