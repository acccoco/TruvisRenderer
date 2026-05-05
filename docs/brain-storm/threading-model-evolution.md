# 线程模型演进：Update Thread 与 Asset Streaming Thread

> 前置文档：`render-thread-isolation.md`（当前双线程模型的落地细节）

## 现状分析

### 当前线程拓扑

```
Main Thread (winit)
  │ crossbeam channel (InputEvent)
  │ AtomicU64 (size)  /  AtomicBool (exit, render_finished)
  ▼
Render Thread
  │ crossbeam channel (AssetLoadRequest → LoadResult)
  ▼
AssetDispatch Thread ──▶ Rayon Worker Pool (Asset-Loader-0..N)
```

- Main Thread 只做事件收集，不碰 Vulkan API。
- Render Thread 串行执行所有帧 phase（input → update → prepare → render → present）。
- AssetLoader 已经有独立 dispatch thread + rayon worker pool 做 IO/decode。
- AssetUploadManager 的 cmd record + submit 仍然发生在 Render Thread 上。

### Render Thread 单帧串行工作

```
begin_frame
  ├─ wait fif timeline (GPU sync, 可能阻塞)
  ├─ reset cmds/resources
  ├─ bindless.begin_frame()
  └─ asset_hub.update()           ← 检查 IO 完成、提交 GPU upload、register 资源
phase_input
  └─ drain events + imgui forward
phase_update
  ├─ update_frame_settings()
  ├─ acquire_image()              ← vkAcquireNextImageKHR
  ├─ build_ui() + compile_ui()
  ├─ gui.prepare_render_data()
  ├─ camera_controller.update()
  └─ plugin.update(UpdateCtx)     ← CPU 场景逻辑
phase_prepare
  ├─ update_accum_frames()
  └─ before_render()
      ├─ scene.prepare_render_data()  ← 遍历全场景构建 RenderData
      ├─ gpu_scene.upload()           ← cmd record + submit
      └─ update_perframe_desc()
phase_render
  └─ plugin.render(RenderCtx)     ← RenderGraph build + compile + execute + submit
phase_present
  ├─ present_image()
  └─ end_frame()
```

瓶颈：所有工作排在一条线程上。`scene.prepare_render_data()` 遍历大场景、`plugin.update()` 做物理计算时，会直接拖慢帧率。

---

## 方案 A：独立 Update Thread

### 核心思想

将 CPU 逻辑（场景更新、物理、动画、UI 构建）和 GPU 工作（命令录制、提交、present）分离，实现流水线并行。

### 流水线时序

```
                Frame N              Frame N+1            Frame N+2

Update Thread: ┌──────────────┐    ┌──────────────┐    ┌──────────────┐
               │ plugin.update│    │ plugin.update│    │ plugin.update│
               │ scene.prepare│    │ scene.prepare│    │ scene.prepare│
               │ build_ui     │    │ build_ui     │    │ build_ui     │
               └──────┬───────┘    └──────┬───────┘    └──────┬───────┘
                      │ produce           │ produce           │
                      ▼                   ▼                   ▼
                 FramePacket N       FramePacket N+1     FramePacket N+2

Render Thread:      ┌──────────────┐    ┌──────────────┐
                    │ gpu upload   │    │ gpu upload   │
                    │ record cmds  │    │ record cmds  │
                    │ submit+prsnt │    │ submit+prsnt │
                    └──────────────┘    └──────────────┘
                     consume N           consume N+1
```

### FramePacket 数据结构

两个线程之间传递不可变的帧快照：

```rust
struct FramePacket {
    render_data: OwnedRenderData,       // 场景快照（owned，非引用）
    camera_transform: CameraTransform,
    pipeline_settings: PipelineSettings,
    accum_data: AccumData,
    delta_time_s: f32,
    total_time_s: f32,
    ui_draw_data: OwnedDrawData,        // imgui draw lists 的 owned 版本
}
```

### 所有权变化

```
              当前                              Update Thread 方案
         ═══════════                        ════════════════════════

World    Render Thread 独占                  Update Thread 独占
         plugin.update(&mut World)           plugin.update(&mut World)

RenderW  Render Thread 独占                  Render Thread 独占
         gpu_scene.upload(...)               gpu_scene.upload(frame_packet)

交接点    无（同一线程）                       FramePacket (channel 传递)

延迟     0 帧                                +1 帧 (渲染上一帧的 CPU 状态)
```

### 当前代码中的障碍

1. **`RenderData<'a>` 持有引用** — `prepare_render_data(&'a self, ...)` 的生命周期绑定到 SceneManager。跨线程传递需要改为 owned 数据或 Arc。

2. **`World` 和 `RenderWorld` 都在 `Renderer` 里** — Rust 借用规则阻止把同一 struct 的两个字段 `&mut` 到不同线程。需要拆分所有权，让 Update Thread 持有 World，Render Thread 持有 RenderWorld。

3. **`scene.prepare_render_data()` 跨 World/RenderWorld 边界** — 需要同时读 `BindlessManager`（RenderWorld）和 `AssetHub`（World）。拆线程后需要重新设计这个边界，可能在 FramePacket 构建时把 bindless handle 快照一并带走。

### 收益与代价

| 收益 | 代价 |
|------|------|
| CPU 更新和 GPU 渲染流水线并行 | +1 帧输入延迟 |
| 复杂物理/AI 不阻塞渲染帧率 | FramePacket 的数据拷贝开销 |
| 更好的帧时间稳定性 | World/RenderWorld 所有权彻底拆分 |
| | RenderData 需要改成 owned |
| | UI 输入响应延迟增加 |
| | 调试复杂度显著增加 |

### 评估

当前项目场景复杂度有限（demo 级别），Update Thread 的收益不明显。这种模式更适合大规模游戏引擎（Unreal、Unity 都采用类似方案）。**优先级：低。**

---

## 方案 B：独立 Asset Streaming Thread

### 当前资产上传的串行卡点

IO 加载已经异步（rayon pool），但 GPU 上传仍在 Render Thread 上同步执行：

```
AssetLoader (异步)                    AssetUploadManager (同步)
┌──────────────────────┐             ┌─────────────────────────────┐
│ rayon pool           │  channel    │ upload_texture()            │
│ file read + decode   ├────────────▶│   ← 在 Render Thread 上    │
│                      │  LoadResult │   同步调用 cmd record+submit│
└──────────────────────┘             │                             │
                                     │ update() 检查完成状态       │
                                     │ register_image/register_srv │
                                     │   也在 Render Thread 上     │
                                     └─────────────────────────────┘
```

问题：
- `upload_texture()` 的 cmd record + submit 虽然提交到 Transfer Queue，但录制本身是同步阻塞 Render Thread 的。
- 大量纹理同帧完成 IO 时，`AssetHub::update()` 会连续提交多个独立 upload，每个一次 submit。
- `register_image` / `register_srv` 操作发生在 Render Thread 的帧循环关键路径上。

### 改进方案

#### B-1: Batched Upload（最小改动）

将同帧完成的多个 IO 结果合并到一个 cmd buffer 提交：

```
当前:
  texture_0: [record cmd_0] [submit]     ← submit ×3
  texture_1: [record cmd_1] [submit]
  texture_2: [record cmd_2] [submit]

改进:
  batch: [record cmd: copy_0, copy_1, copy_2] [submit]  ← submit ×1
```

代码中已有 TODO（`asset_upload_manager.rs`）：
> `// TODO image 的 upload，可以考虑每帧合并多个 upload 任务到同一个 Command Buffer 中提交`

改动范围仅限 `AssetUploadManager` 内部，公开接口不变。

#### B-2: 独立 Staging Thread

将 staging buffer 分配、cmd 录制、Transfer Queue 提交移到独立线程：

```
IO Pool (rayon)
  │ RawAssetData (channel)
  ▼
Staging Thread (新增)
  ├─ batch 收集本轮完成的 RawAssetData
  ├─ allocate staging buffers
  ├─ memcpy pixels → staging
  ├─ record ONE cmd buffer (batched copies)
  └─ submit to Transfer Queue + signal timeline semaphore
        │
        │ timeline semaphore (跨队列同步)
        ▼
Render Thread (每帧 poll)
  ├─ check timeline semaphore value (非阻塞)
  └─ for each completed batch:
      ├─ register_image (GfxResourceManager)
      ├─ register_srv (BindlessManager)
      └─ update texture state → Ready
```

关键点：
- Transfer Queue 和 Graphics Queue 是独立的硬件单元（现代 GPU 上 DMA 不占 shader 核心），可以真正并行。
- 跨队列同步用 Timeline Semaphore（当前已有基础设施）。
- Render Thread 只做轻量的 `register` 操作（poll semaphore + 更新 handle table），不再录制 copy cmd。

### 改动评估

| 改动 | 难度 | 说明 |
|------|------|------|
| Batched Upload (B-1) | 低 | 纯 `AssetUploadManager` 内部改动，接口不变 |
| Staging Thread (B-2) | 中 | `upload_texture` 从同步调用改为发消息；staging buffer 生命周期用 `PendingUpload` 模式（已有）管理 |
| register 推迟到 Render Thread | 低 | 当前已经如此（`update()` 里做） |
| 跨队列 semaphore | 低 | 当前已有 timeline semaphore，只需在 graphics submit 增加 wait |

### 评估

Batched Upload 改动极小、收益确定，应该优先做。Staging Thread 在当前基础设施上补全不算大改动，适合在资产规模增长后推进。**优先级：Batched Upload 高，Staging Thread 中。**

---

## 如果两者都实现：理想线程拓扑

```
┌──────────────┐   events    ┌──────────────┐
│ Main Thread  │ ──────────▶ │ Update Thread│
│ (winit)      │             │ input/scene  │
└──────────────┘             │ physics/UI   │
                             └──────┬───────┘
                                    │ FramePacket (channel)
                                    ▼
┌──────────────┐             ┌──────────────┐
│ IO Pool      │  channel    │ Render Thread│
│ (rayon)      │ ──────────▶ │ gpu upload   │ ◀── Graphics Queue
│ file decode  │             │ cmd record   │
└──────────────┘             │ submit/prsnt │
                             └──────────────┘
┌──────────────┐  timeline          ▲
│ Staging      │  semaphore         │
│ Thread       │ ───────────────────┘
│ transfer cmd │ ◀── Transfer Queue
└──────────────┘

线程数: 1 main + 1 update + 1 render + 1 staging + N rayon workers
```

---

## 建议优先级

| 优先级 | 方案 | 理由 |
|--------|------|------|
| **高** | B-1: Batched Upload | 改动极小，收益确定，已有 TODO |
| **中** | B-2: Staging Thread | 架构基础设施已有（Transfer Queue + Timeline Semaphore），补全不算大改动 |
| **低** | A: Update Thread | 需要 World/RenderWorld 所有权彻底拆分、RenderData 改 owned、引入 FramePacket，工程量大，当前场景复杂度下收益有限 |

## 参考

- 当前线程模型落地细节：`render-thread-isolation.md`
- World/RenderWorld 拆分进展：`openspec/changes/split-render-context-world-renderworld/`
- Vulkan Transfer Queue 并行：GPU 厂商（NVIDIA/AMD）文档中关于 async compute 和 async transfer 的说明
