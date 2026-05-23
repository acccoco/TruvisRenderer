# render-passes 对 truvis-world 的依赖解耦

> 日期：2026-05-04
> 状态：探索中
> 关联：[clean-crate-dependencies.md](./clean-crate-dependencies.md)

## 一、问题描述

`truvis-render-passes` 按分层设计应只依赖 `truvis-render-interface`（L2），
但实际 Cargo.toml 中声明了对 `truvis-world`（L3 聚合体）的依赖。

这是上一轮依赖清理（render-graph / gui-backend 解耦）后遗留的同类问题。

## 二、影响分析

### 2.1 唯一使用点

整个 crate 中只有 `phong_pass.rs` 一处引用了 `truvis_world::World`：

```rust
// phong_pass.rs:20
use truvis_world::World;

// phong_pass.rs:101
pub fn draw(&self, cmd: &GfxCommandBuffer, gpu_store: &GpuStore, world: &World) {
```

用法（第 139-143 行）：

```rust
gpu_store.gpu_scene.draw(
    cmd,
    &world
        .scene_manager
        .prepare_render_data(&gpu_store.bindless_manager, &world.asset_hub),
    &mut |ins_idx, submesh_idx| { /* push constants */ },
);
```

其余 7 个 pass（blit / accum / denoise_accum / sdr / resolve / realtime_rt）均不依赖 World。

### 2.2 违反的原则

1. **分层违反**：render-passes（L5）反向伸手到 World（L3 聚合体），绕过分层边界。
2. **phase 语义违反**：`prepare_render_data()` 是 CPU 侧数据准备，不应在 render phase（GPU 录制）中执行。
   正常路径是 Renderer::update_gpu_scene()（phase_prepare）提前调用。

### 2.3 当前状态

PhongPass 目前没有被任何上层代码调用（无使用者），改签名不会产生连锁变更。

## 三、方案对比

### 方案 A：传入 RenderData（最小改动）✅ 推荐

将 `&World` 参数替换为 `&RenderData<'_>`（已定义在 render-interface 中的纯数据快照）。

```rust
// Before
pub fn draw(&self, cmd: &GfxCommandBuffer, gpu_store: &GpuStore, world: &World)

// After
pub fn draw(&self, cmd: &GfxCommandBuffer, gpu_store: &GpuStore, scene_data: &RenderData<'_>)
```

调用侧（未来的 app 代码）负责在 prepare phase 构建 RenderData：

```rust
// phase_prepare:
let scene_data = world.scene_manager.prepare_render_data(&bindless_manager, &asset_hub);

// phase_render:
phong_pass.draw(cmd, gpu_store, &scene_data);
```

| 维度         | 评价                                                           |
|-------------|---------------------------------------------------------------|
| 改动量       | ~10 行（改签名 + draw 内部 + 删 Cargo.toml 依赖）               |
| 分层         | ✓ render-passes 完全脱离 World / scene / asset                  |
| phase 语义   | ✓ prepare 在 prepare phase，draw 在 render phase                |
| 一致性       | ✓ 与其他 pass 风格一致（只依赖 render-interface 的类型）          |
| 风险         | 低，PhongPass 当前无调用者                                      |

### 方案 B：将 RenderData 存入 GpuStore

在 phase_prepare 阶段把 RenderData 缓存到 GpuStore 中，
PhongPass 签名简化为 `draw(&self, cmd, gpu_store)` —— 不需要额外参数。

```rust
pub struct GpuStore {
    // ...existing fields...
    pub current_scene_data: RenderData<'static>,  // owned version
}
```

| 维度         | 评价                                                           |
|-------------|---------------------------------------------------------------|
| 改动量       | ~30-50 行（GpuStore 增字段 + prepare 阶段存储 + 生命周期处理） |
| 分层         | ✓ 同样脱离 World                                               |
| 签名简洁度   | 最高，所有 pass 统一只接 &GpuStore                            |
| 生命周期     | ✗ RenderData 当前带 `'a`（MeshRenderData 引用 `&'a [RtGeometry]`），需要改为 owned 或 Arc |
| 风险         | 中等，涉及 GpuStore 结构体改动                                |

## 四、推荐路径

选择 **方案 A**。理由：

1. 改动量最小，且正确解决分层和 phase 语义两个问题。
2. 与其他 pass 的设计风格一致。
3. PhongPass 当前无调用者，不存在连锁影响。
4. 方案 B 的 RenderData 生命周期问题（`'a` → owned）是一个独立的架构决策，
   不应与"去掉 World 依赖"混在一起解决。

## 五、后续可探索

- **RenderData 的 owned 化**：如果未来更多 pass 需要场景数据，
  可以考虑将 `MeshRenderData` 的 geometry 引用改为 Arc 或索引，
  使 RenderData 变为 `'static`，再落实方案 B。
- **PhongPass 是否还需要保留**：它目前无调用者，
  如果光栅化管线不再使用，可以考虑移除或移到 app 层作为 demo pass。
