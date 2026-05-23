# 线程模型演进

> 状态：活跃方向，更新于 2026-05-23。当前线程事实以
> [`ARCHITECTURE.md`](../../ARCHITECTURE.md) 为准。

## 当前线程拓扑

```text
Main Thread (winit)
  -> crossbeam channel: InputEvent
  -> atomics: latest size / exit / render finished
  -> Render Thread
       -> owns RenderAppShell + RenderRuntime + all Vulkan objects
       -> begin_frame / update / prepare / render / present
       -> AssetHub::update polls async CPU load results
       -> render-side uploaders poll GPU upload completion
       -> rayon workers perform asset IO / decode / Assimp CPU import
```

约束：

- 主线程不调用 Vulkan、`ash` 或 `truvis-gfx` API。
- 所有 Vulkan 对象在渲染线程创建、使用和销毁。
- `AssetHub` 后台任务只产出 CPU 数据或失败状态，不创建 GPU 对象。
- GPU upload command recording / submit 当前仍发生在 render thread 或 render-side uploader owner 内。
- Mesh uploader 已使用 pending timeline 轮询，未 ready 的 mesh 不会进入 active instance。

## Render Thread 单帧工作

```text
begin_frame
  -> wait FIF timeline
  -> reset per-frame command/resource cleanup
  -> begin frame for bindless/material/instance managers
  -> AssetHub::update
  -> dispatch AssetLoadedEvent to texture / mesh / material owners

update_phase
  -> update frame settings
  -> acquire swapchain image
  -> App update
  -> Plugin update

prepare
  -> update accum state
  -> bindless descriptor prepare
  -> material texture resolve and upload
  -> instance ready gate and RenderData snapshot
  -> GpuScene upload / TLAS rebuild if dirty
  -> per-frame uniform and descriptor update

render / present
  -> App builds and executes RenderGraph
  -> present
  -> advance frame counter
```

## Option B-1：Batched Upload

目标：把同一帧 ready 的多个 texture upload 合并到更少的 command buffer / submit 中。

适用原因：

- IO / decode 已经异步，但大量资源同帧完成时，render thread 仍可能集中录制和提交多次 upload。
- batching 不改变 AssetHub 对外事件语义，也不改变 render pass 输入。
- 这是当前最小、收益最确定的资产上传优化方向。

约束：

- batching 必须保持每个 asset handle 的完成顺序、失败状态和资源销毁 owner 清晰。
- pending upload 的 staging/image/command 生命周期仍以 timeline 或 frame token 明确保护。
- bindless 注册仍在 render-side owner 完成，AssetHub 不回退到 GPU 资源管理。

优先级：高。

## Option B-2：Staging Thread

目标：将 staging buffer 准备、copy command 录制和 transfer queue submit 移出 render thread。

推荐形态：

```text
rayon IO/decode
  -> upload request batch
  -> staging thread records transfer work
  -> transfer queue signals timeline
  -> render thread polls completion
  -> register resource / bindless / ready state
```

收益：

- 减少 render thread 上的 asset upload 尖峰。
- 可以利用独立 transfer queue 与 graphics queue 并行。

代价：

- 需要明确跨线程 owner、command pool、timeline semaphore 和 shutdown 顺序。
- 需要保证 `GfxResourceManager` / bindless 注册仍只在合法 owner 线程内发生。

优先级：中。适合在 batching 之后、资源规模继续增长时推进。

## Option A：Update Thread

目标：将 CPU update、UI 构建、scene snapshot 与 render thread 的 GPU 工作流水线并行。

需要前提：

- `World` 与 render-side owner 的所有权真正跨线程拆分。
- update thread 产出 owned frame packet，不能携带借用到 `SceneManager` 或 uploader 内部缓存。
- 输入、UI、camera、pipeline settings 和 scene snapshot 都要接受至少一帧延迟。
- shutdown / panic propagation / resize 需要新的跨线程协议。

当前评估：

- 对 demo 级场景收益有限。
- 工程量和调试复杂度高。
- 在显式 extract、owned scene snapshot 和 PluginGroup 之前不建议推进。

优先级：低。

## 历史来源

- 当前双线程落地细节见 [`archive/render-thread-isolation.md`](archive/render-thread-isolation.md)。
- 早期线程方案已被本文更新，不再保留旧 AssetDispatch Thread 描述。
