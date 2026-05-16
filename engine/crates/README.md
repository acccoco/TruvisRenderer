# Engine Crates

`engine/crates/` 存放引擎 Rust 核心 crate。该目录是运行时框架、渲染管线与场景系统的主要实现区域。

## 关键主干

`truvis-gfx` -> `truvis-render-interface` / `truvis-world` -> `truvis-render-graph` / `truvis-render-passes` -> `truvis-render-backend` -> `truvis-frame-api` / `truvis-frame-runtime` -> `truvis-app`

## 主要模块

- `truvis-gfx`：Vulkan RHI 抽象
- `truvis-render-interface`：`RenderWorld`、帧状态、资源句柄与 GPU 资源管理原语
- `truvis-world`：CPU scene/assets 容器
- `truvis-render-graph`：按 pass 添加顺序执行的命令录制与同步辅助
- `truvis-render-backend`：backend 执行与系统整合（swapchain/cmd/sync/submit/present + GPU 上传）
- `truvis-frame-api`：`RenderApp` / `RenderAppHooks` / `Plugin` 契约与 Plugin typed contexts
- `truvis-frame-runtime`：`RenderAppShell` 帧骨架与 render-loop 适配层
- `truvis-render-passes`：通用 render pass 实现（RT / 累积 / 降噪 / 色调映射 / blit / resolve / phong）
- `truvis-app`：示例应用、GUI plugin、overlay plugin 与 render pipeline plugin 编排
- `truvis-scene` / `truvis-asset`：场景与资产系统
- `truvis-gui-backend`：ImGui Vulkan 后端
- `truvis-logs`：日志初始化与线程上下文 formatter

## 协作建议

- 新功能优先复用现有层次，不跨层直接依赖。
- 涉及多模块改动时先确认依赖方向是否仍是 DAG。
- 具体边界约束见 `../../ARCHITECTURE.md`。
