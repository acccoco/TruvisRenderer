# 架构原则与开放问题

> 状态：活跃摘要，更新于 2026-05-23。当前事实以
> [`ARCHITECTURE.md`](../../ARCHITECTURE.md) 和代码为准。

本文提炼历史架构诊断中仍有效的原则，避免在多个旧草案中重复查找。

## 当前边界

- `truvis-winit-app` 只负责 winit 事件循环、窗口生命周期、输入转发、resize/exit 信号和渲染线程启动。
- `truvis-app-frame` 提供 `RenderApp` 契约、`RenderAppShell` 帧骨架、`RenderAppHooks` 和标准 `Plugin` 生命周期。
- `truvis-render-runtime` 是 GPU 运行时集成层，拥有 `World`、`GpuStore`、runtime 私有 `GpuScene`、present、cmd、sync、asset manager 与 scene bridge。
- `truvis-world` 是 CPU 语义层，聚合 runtime scene 和 asset hub，不拥有 Vulkan、swapchain、GPU buffer/image 或 frame state。
- `truvis-render-foundation` 是渲染基础层，提供 `GpuStore`、manager、frame state、global descriptors、FIF 资源和 `RenderSceneView` 等底层契约。
- `truvis-render-graph` 只做图编排和线性同步推导，不承载 scene / asset 领域对象。
- `truvis-render-passes` 只依赖 render-side 只读视图和 GPU 状态，不回到 CPU world 取数据。
- `app-kit` 和具体 app 负责组合 GUI、camera/input、overlay、RT / Shadertoy / triangle 等业务能力。

## 设计原则

- 下层不反向依赖上层业务；同层 crate 默认不互相依赖，除非架构文档明确允许。
- 数据 owner 和执行 owner 分开表达：CPU 语义在 `World`，GPU-facing 状态在 `GpuStore` / runtime 私有 owner。
- App / Plugin 只拿当前 phase 所需的 typed Ctx，不长期保存 `Gfx`、runtime 内部字段或 manager 借用。
- RenderGraph pass 只声明资源读写和录制命令，不在 render phase 做 scene extract 或 asset resolve。
- CPU asset handle、GPU resource handle、shader-visible bindless handle 是三种不同身份，不跨层混用。
- `RenderData` 是 update 与 render 之间的快照产物，不能重新变成可被所有模块访问的大上下文。
- 注入和显式参数优先于全局访问；确实需要全局寿命的对象也应通过 owner 控制销毁顺序。

## 开放问题

- 显式 `extract -> prepare -> render`：当前 `RenderRuntime::prepare()` 仍是单入口，需要进一步拆出 CPU snapshot、GPU upload、descriptor/per-view update 的语义边界。
- View 抽象：当前主视角仍由 camera、frame settings、per-frame data 和 FIF resources 隐式组合，尚未形成 `ViewDesc` / `PreparedView`。
- Plugin 装配：当前由 App 显式字段组合能力，尚未支持 `PluginGroup`、依赖声明、拓扑校验和 builtin plugin。
- Surface 边界：present/swapchain 仍由 runtime 持有，`SurfaceRegistry`、多窗口和 headless 仍是远期方向。
- `Gfx` 注入：部分底层代码仍有历史全局访问痕迹，后续应继续向显式 owner / typed ctx 收敛。
- app-kit 拆分：GUI、camera/input、overlay 和 pipeline glue 可继续拆为更清晰的可复用 feature。
- 资产上传并发：texture / mesh 上传已经从 AssetHub 移出，但 batched upload 和 staging thread 仍可继续评估。

## 历史来源

本文提炼自以下归档文档：

- [`archive/2026-04-22-architecture-evolution-gap-analysis.md`](archive/2026-04-22-architecture-evolution-gap-analysis.md)
- [`archive/2026-04-23-structure-responsibility-open-source-comparison.md`](archive/2026-04-23-structure-responsibility-open-source-comparison.md)
- [`archive/ideal_layered_architecture.md`](archive/ideal_layered_architecture.md)
- [`archive/ideal-module-architecture.md`](archive/ideal-module-architecture.md)
- [`archive/render-app-layering-analysis.md`](archive/render-app-layering-analysis.md)
