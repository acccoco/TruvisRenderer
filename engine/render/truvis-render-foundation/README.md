# truvis-render-foundation

渲染基础层，提供 GPU 资源句柄、全局描述符、FIF 索引与 GPU 资源管理通用原语。

## 关键组件

- `FrameCounter` / `FrameLabel` / `FrameToken`
- `CmdAllocator`
- `GfxResourceManager`（Handle + 生命周期管理）
- `ShaderBindingSystem`（`GlobalDescriptorSets` + `BindlessManager` + sampler manager）
- `PerFrameGpuData`
- `RenderView`
- `RenderSceneView`

## 基础 GPU Owner 与契约

- `GfxResourceManager` 是 manager-owned GPU image/buffer/view 的生命周期 owner，不包装进额外聚合。
- `ShaderBindingSystem` 是 shader-visible binding owner，聚合 `GlobalDescriptorSets`、`BindlessManager` 和 sampler manager，并提供 bindless 注册/注销与 render 阶段只读 view。
- `FrameCounter` / `FrameLabel` / `FrameToken` 只表达 FIF slot、帧序号 token 和延迟回收窗口；当前帧时间快照由 `truvis-render-runtime::FrameTiming` 持有。
- `PerFrameGpuData` 持有 per-FIF `PerFrameData` UBO，负责当前帧 GPU 常量写入和 device address 查询。
- CPU scene 或 asset hub 属于 `truvis-world::World`，不进入这些 GPU-facing owner。
- `FrameRenderState`、`RenderOptions`、`ViewAccumState`、`DlssSrState` 和 `FrameTiming` 属于 `truvis-render-runtime`；foundation 不承载 runtime-owned render state。
- render 阶段由 `truvis-render-runtime` 借出 `RenderPassRecordCtx`，foundation 只提供该上下文引用的基础 owner、FIF 索引和视图契约。
- resize / shutdown 阶段通过 mutable lifecycle context 暴露 `GfxResourceManager` 与 `ShaderBindingSystem`，用于重建、注册或释放 manager-owned GPU resources。
- `GlobalDescriptorSets` 只作为全局 pipeline 绑定聚合；资源 manager 更新 descriptor 时只能接收专用 target，避免依赖完整全局绑定策略。

## RenderSceneView

- `RenderSceneView` 是 render pass 访问 GPU scene 的窄只读契约。
- concrete `GpuScene`、`RenderData`、稳定 instance slot 与 raster draw cache 属于 `truvis-render-runtime` 私有实现。
- render pass 只能通过 `RenderSceneView` 读取 scene root buffer device address、当前 FIF TLAS handle，并提交光栅化 draw；不直接依赖 runtime 私有场景上传数据。

## 资源管理规则

- 本层 API 通过 typed `Gfx` Ctx 接收底层能力；runtime owner 和长期资源字段不保存 Ctx 引用。
- `GfxResourceManager` 是 manager-owned image/view 的唯一释放入口，负责 view 先于 image 销毁。
- 延迟销毁通过 frame id 入队，`cleanup()` 到达安全帧后释放，并记录 `DestroyReason::DeferredCleanup`。
- resize / shutdown / immediate release 必须使用带 `DestroyReason` 的 release API，便于把日志关联到项目资源名、raw Vulkan handle 与 manager handle。
- 具体窗口尺寸 render target（如 RT working target、main view target、GBuffer）由 app 层具体 pipeline/plugin 管理；foundation 只提供 handle、manager、frame label 和 bindless 注册能力。
- `BindlessManager`、`RenderSamplerManager` 等 manager 只依赖自身 descriptor binding 契约和窄 target，不以 `GlobalDescriptorSets` 作为更新入口。
- `CmdAllocator`、`GlobalDescriptorSets`、`RenderSamplerManager` 等 owner 在 shutdown 时接收 phase Ctx 并显式销毁自身持有的 GPU 资源。
- `Drop` 只保留诊断职责，不作为 Vulkan/VMA 释放路径。

## 模块定位

- 位于 RHI 与上层渲染逻辑之间
- 提供稳定的数据契约，减少上层直接触碰底层细节
- 不依赖 App、Plugin、scene loading、窗口平台或 runtime render state 语义
