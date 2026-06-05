# truvis-render-foundation

渲染基础层，提供 GPU 帧状态、资源句柄、全局描述符与 GPU 资源管理通用原语。

## 关键组件

- `FrameCounter` / `FrameLabel`
- `FrameRenderState` / `RenderOptions` / `ViewAccumState`
- `DlssSrMode` / `DlssSrState`
- `CmdAllocator`
- `GfxResourceManager`（Handle + 生命周期管理）
- `BindlessManager` / `GlobalDescriptorSets`
- `RenderSceneView`
- `GpuStore`

## GpuStore

- `GpuStore` 是 GPU 侧运行时状态集合，包含 `BindlessManager`、`GlobalDescriptorSets`、`GfxResourceManager`、sampler manager、per-frame data、frame counter、`FrameRenderState`、`RenderOptions` 和 `ViewAccumState`。
- `RenderOptions` 只保存 runtime 全局可调选项；RT debug channel、legacy denoise 参数和实验性 IC 开关属于具体 pipeline/pass，不放入 foundation 全局状态。
- `FrameRenderState` 是 runtime 派生的 main view 帧状态，记录 HDR format、depth format、render extent 和 output extent；调用方读取它创建 target，但不把它当作用户配置。
- `ViewAccumState` 是当前 main view 的 temporal state，不是配置项；resize、view 变化或环境切换会让 runtime 重置它。
- `GpuStore` 不包含 CPU scene 或 asset hub；这些数据属于 `truvis-world::World`。
- render 阶段通常只借出 `&GpuStore`，使 pass adapter 能读取 GPU 状态并录制命令，但不能随意改写 frame state。
- resize / shutdown 阶段通过 mutable context 暴露 `GpuStore`，用于重建或释放 manager-owned GPU resources。
- `GlobalDescriptorSets` 只作为全局 pipeline 绑定聚合；资源 manager 更新 descriptor 时只能接收专用 target，避免依赖完整全局绑定策略。

## RenderSceneView

- `RenderSceneView` 是 render pass 访问 GPU scene 的窄只读契约。
- concrete `GpuScene`、`RenderData`、稳定 instance slot 与 raster draw cache 属于 `truvis-render-runtime` 私有实现。
- render pass 只能通过 `RenderSceneView` 读取 scene root buffer device address、当前 FIF TLAS handle，并提交光栅化 draw；不直接依赖 runtime 私有场景上传数据。

## 资源管理规则

- 本层 API 通过 typed `Gfx` Ctx 接收底层能力；`GpuStore` 和长期资源字段不保存 Ctx 引用。
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
- 不依赖 App、Plugin、scene loading 或窗口平台语义
