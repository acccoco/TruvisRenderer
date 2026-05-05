## Context

经过四次架构重构，系统已具备 FrameRuntime 调度 + AppPlugin 契约 + 物理 crate 分层的基础。但核心数据容器 `RenderContext` 仍是一个混合了 CPU 场景状态和 GPU 渲染状态的单一结构体，阻碍了 phase 边界的类型化表达和依赖层次的进一步收窄。

当前分层（简化）：

```
L2  render-interface (GPU 边界类型)
L3  render-graph (pass 编排) ← FifBuffers 放错在这里
L4  renderer (RenderContext 定义在这里)
L5  render-passes → renderer (仅为 RenderContext 类型)
    app-api → renderer (为 RenderContext + Camera + RenderPresent)
```

已做出的用户决策：
- Shared 帧状态字段（FrameCounter、Settings、time 等）放在 RenderWorld 中
- 新建 `truvis-world` crate 持有 World，RenderWorld 定义在 `render-interface`
- AppPlugin contexts 采用 World / RenderWorld 分离模型
- UpdateCtx 直接给 `&mut World`
- 删除 `RenderContext2`（`&RenderWorld` 已满足只读需求）

## Goals / Non-Goals

**Goals:**
- 将 `RenderContext` 拆分为 `World`（CPU 场景状态）和 `RenderWorld`（GPU 渲染状态 + 帧状态），建立物理分离的所有权边界
- `FifBuffers` 从 `render-graph` 迁移到 `render-interface`，修正放错位置的类型
- AppPlugin phase contexts 通过类型约束体现 CPU/GPU 的 phase 边界
- `render-passes` 去掉对 `renderer` 的依赖，降到 L3
- 全部 demo app 和 pass 无功能回归

**Non-Goals:**
- 不引入 Extract 作为显式帧阶段（后续 P1）
- 不将 Camera / Input / GUI 从 FrameRuntime 解耦为 Plugin（后续 P2）
- 不改变 Renderer 内部的调度流程（begin_frame → update_assets → before_render 调用顺序不变）
- 不改变渲染线程模型
- 不做通用 ECS 或 TypeMap 式 World 容器

## Decisions

### Decision 1: RenderWorld 定义在 truvis-render-interface

**选择**: 在 `truvis-render-interface` 中新增 `render_world.rs`

**理由**: RenderWorld 的全部字段（GpuScene、BindlessManager、GlobalDescriptorSets、GfxResourceManager、SamplerManager、PerFrameDataBuffers、FrameCounter、FrameSettings 等）已经定义在 render-interface 中。唯一的例外是 FifBuffers（在 render-graph），但 FifBuffers 仅依赖 render-interface 和 gfx 的类型，可以先迁入。

**备选方案**:
- 定义在 `truvis-renderer`：最小改动，但 render-passes 仍需依赖 renderer，无法降层
- 新建独立 crate：过度拆分，收益不大

**前置条件**: FifBuffers 必须先迁移到 render-interface。

### Decision 2: 新建 truvis-world crate

**选择**: 新建 `engine/crates/truvis-world`，依赖 `truvis-scene` + `truvis-asset`

**理由**: World 包含 SceneManager（来自 truvis-scene）和 AssetHub（来自 truvis-asset）。如果定义在 render-interface，会让 L2 反向依赖 L3 的 scene/asset。独立 crate 保证依赖方向清晰。

**结构**:

```
truvis-world (L3, 新 crate)
  ├─► truvis-scene
  └─► truvis-asset
```

**备选方案**:
- 定义在 `truvis-renderer`：renderer 已经依赖 scene + asset，但 World 概念不属于渲染后端
- 不建 crate，仅在 renderer 内部用结构体分组：无法被 app-api 独立引用

### Decision 3: FifBuffers 迁移到 render-interface

**选择**: 将 `FifBuffers` 从 `truvis-render-graph::resources::fif_buffer` 迁移到 `truvis-render-interface`

**理由**: FifBuffers 的全部依赖（GfxImage/Handle、BindlessManager、FrameCounter、GfxResourceManager、FrameSettings）均来自 render-interface 和 gfx。它与 render graph 的概念（pass、dependency graph、barrier 推导）无关。源码中已有 `// TODO FifBuffers 放到 app 里面去` 的标注。

**迁移方式**: 移动源文件，render-graph 中保留 re-export 以减少下游立即破坏（可选的过渡期兼容），后续统一清理。

### Decision 4: AppPlugin Contexts 分离模型

**选择**:

| Context | 给什么 | Phase 语义 |
|---------|--------|-----------|
| `InitCtx` | `&mut World` + `&mut RenderWorld` + Camera + CmdAllocator + RenderPresent | 一次性初始化，两侧都可写 |
| `UpdateCtx` | `&mut World` + `&mut PipelineSettings` + `&FrameSettings` + delta_time | CPU 更新，可写 World |
| `RenderCtx` | `&RenderWorld` + RenderPresent + DrawData + Timeline | GPU 录制，RenderWorld 只读 |
| `ResizeCtx` | `&mut RenderWorld` + RenderPresent | Swapchain 重建后的 GPU 资源重建 |

**UpdateCtx 给 `&mut World` 的理由**: 虽然当前 demo 只改 SceneManager，但 World 只有两个字段（scene_manager + asset_hub），限制更窄的暴露面不如直接给 World 来得一致和可扩展。

**PipelineSettings 在 UpdateCtx 中单独借出**: 虽然 PipelineSettings 在 RenderWorld 中，但 Update phase 是 CPU 阶段，plugin 只应修改 settings 而非 GPU 资源。从 RenderWorld 中单独借出一个 `&mut PipelineSettings` 和一个 `&FrameSettings` 是安全的（Rust 字段级借用）。

### Decision 5: 不引入 RenderWorld 方法，保持字段级访问

**选择**: RenderWorld 只是 plain struct（公开字段），不添加方法。Renderer 中的 `update_gpu_scene` / `update_perframe_descriptor_set` 继续直接访问字段。

**理由**: 如果 RenderWorld 有 `&mut self` 方法来做上传，Rust 借用检查器会阻止同时借用多个字段（如 `&mut gpu_scene` + `&frame_counter` + `&bindless_manager`）。保持字段级访问利用 Rust 的 disjoint field borrowing 特性，避免不必要的借用冲突。

## Risks / Trade-offs

**[Risk] 大量文件需要同步修改** → Mitigation: 分步实施，每步 cargo check 验证。步骤间保持编译通过。FifBuffers 迁移、RenderWorld 定义、Renderer 重构、context 更新、pass 迁移各为独立可验证步骤。

**[Risk] render-graph re-export FifBuffers 可能造成混乱** → Mitigation: re-export 标注 `#[deprecated]`，在本次变更中同步更新 render-graph 内部的 FifBuffers 引用路径。

**[Risk] UpdateCtx 给 &mut World 后，pipeline_settings 和 frame_settings 需要从 render_world 中单独借出** → Mitigation: Rust 字段级借用允许同时持有 `&mut renderer.world` 和 `&mut renderer.render_world.pipeline_settings`，因为它们是不同字段路径。FrameRuntime 构造 UpdateCtx 时直接分别借出即可。

**[Risk] render-passes 去掉 renderer 依赖后，如果未来 pass 需要 renderer 中的新类型怎么办** → Mitigation: 如果未来需要，应评估该类型是否属于 render-interface 层。render-passes 依赖 renderer 是架构逆向，应避免。

**[Trade-off] 新增一个 crate (truvis-world)**: 增加了项目的 crate 数量，但换来了干净的依赖方向。World 只有两个字段，类型很薄，但作为独立 crate 可以被 app-api 和 frame-runtime 直接引用而不穿透 renderer。
