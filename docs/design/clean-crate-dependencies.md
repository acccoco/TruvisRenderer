# Engine Crate 依赖清理

> 日期：2026-04-11
> 状态：已完成

## 一、改动原因

对 `engine/crates/` 的 crate 依赖图进行审查后，发现三处层次违反和一处无用依赖：

### 1.1 `truvis-render-graph` 向上依赖了 `truvis-scene` 和 `truvis-asset`

`RenderContext`（聚合了渲染期间所有状态的 "God struct"）被定义在 `truvis-render-graph` 中，它包含 `SceneManager`（来自 scene）和 `AssetHub`（来自 asset）两个字段。这导致本应是纯粹 pass 编排层的 render-graph 被迫知道领域模型的具体类型。

**影响：**
- render-graph 无法脱离 scene/asset 单独使用（比如纯 compute post-processing pipeline）
- 任何 scene/asset 的改动都触发 render-graph 重编译
- 层次关系名存实亡——文档说 `gfx → interface → graph → renderer`，但实际 graph 已经"知道"了 renderer 层才该知道的东西

### 1.2 `truvis-gui-backend` 依赖了 `truvis-render-graph`

`gui_pass.rs` 中定义了 `GuiRgPass`——一个将 `GuiPass`（纯 Vulkan 录制）包装成 render graph pass 的适配器。这个适配器 `impl RgPass` 并持有 `&RenderContext`，强行把 gui-backend 拉入了 render-graph 的依赖树。

**影响：**
- gui-backend 的职责边界模糊（既是"纯 Vulkan 后端"又是"render graph 参与者"）
- gui-backend 无法在不引入 render-graph 的上下文中使用

### 1.3 `truvis-logs` 存在 5 个幽灵依赖

`truvis-logs` 的 `Cargo.toml` 中声明了 `reqwest`、`serde`、`zip`、`toml`、`anyhow`，但源码中完全没有使用它们。整个 crate 只有一个 40 行的 `lib.rs`，只调用了 `env_logger`、`log`、`anstyle`、`chrono`。

**影响：**
- `reqwest` 的依赖树极深，显著拖慢编译速度
- 依赖图中存在无意义的噪声

## 二、改动方案对比

### 2.1 RenderContext 的归属问题

| 方案 | 描述 | 优点 | 缺点 |
|------|------|------|------|
| **A: 整体搬迁到 renderer** ✅ | 将 `RenderContext` 原样从 render-graph 搬到 renderer，render-graph 内部的 `ComputePass::exec` 改为接收具体参数 | 最简单，改动量最小 | 没有引入新的抽象层 |
| B: 拆分为 RgContext + RenderContext | render-graph 定义精简的 `RgContext`（只含 frame_counter 等），renderer 组装完整的 `RenderContext` 包含 `RgContext` 作为字段 | 给 render-graph 一个自己的上下文类型 | 引入新类型和嵌套关系，增加复杂度；render-graph 中只有 `ComputePass::exec` 一个地方用了 RenderContext |
| C: trait 抽象 | render-graph 定义 `trait RenderContextProvider`，renderer 实现 | 最大程度解耦 | 过度抽象，增加泛型/trait dispatch 复杂度 |

**选择方案 A 的理由：** `ComputePass::exec` 是 render-graph 内部**唯一**使用 `RenderContext` 的代码，且只用到 `frame_counter` 和 `global_descriptor_sets` 两个字段。直接改参数列表（从 `&RenderContext` 改为 `FrameLabel` + `&GlobalDescriptorSets`）比引入新类型或 trait 更简单、更直接。

### 2.2 GuiRgPass 的归属问题

| 方案 | 描述 | 优点 | 缺点 |
|------|------|------|------|
| **A: 搬到 truvis-app** ✅ | 将 `GuiRgPass` 从 gui-backend 搬到 app，同时重构 `GuiPass::draw` 签名 | 跟随使用者；所有 `GuiRgPass` 的使用点都在 app | 不如 renderer "正统" |
| B: 搬到 truvis-renderer | 将 `GuiRgPass` 搬到 renderer | renderer 负责组装 render graph，更正统 | renderer 中没有代码直接使用 `GuiRgPass`；所有使用者在 app |

**选择方案 A 的理由：** `GuiRgPass` 的所有使用者（`triangle_app`、`shader_toy_app`、`rt_render_graph`）都在 `truvis-app`。让适配器跟随使用者更自然。同时，为了让 gui-backend 彻底不依赖 `RenderContext`（因为 gui-backend 不能反向依赖 renderer），还重构了 `GuiPass::draw` 的签名，将 `&RenderContext` 替换为显式参数。

## 三、具体改动

### 3.1 truvis-logs：删除幽灵依赖

从 `Cargo.toml` 中删除 `reqwest`、`serde`、`zip`、`toml`、`anyhow` 五行。零代码变更。

### 3.2 RenderContext 搬迁 + ComputePass 解耦

1. 在 `truvis-renderer/src/render_context.rs` 中创建 `RenderContext` 和 `RenderContext2`（从 render-graph 原样搬入，import 路径调整为外部 crate 引用）
2. 修改 `ComputePass::exec` 签名：
   ```
   // BEFORE (in truvis-render-graph)
   fn exec(&self, cmd, render_context: &RenderContext, params, group_cnt)

   // AFTER
   fn exec(&self, cmd, frame_label: FrameLabel, global_descriptor_sets: &GlobalDescriptorSets, params, group_cnt)
   ```
3. 删除 `truvis-render-graph/src/render_context.rs`
4. 从 render-graph 的 `Cargo.toml` 移除 `truvis-scene` 和 `truvis-asset`
5. 更新所有引用路径（renderer、app 中约 13 个文件）

### 3.3 GuiRgPass 分离 + GuiPass::draw 重构

1. 在 `truvis-app/src/gui_rg_pass.rs` 中创建 `GuiRgPass`（从 gui-backend 搬入）
2. 重构 `GuiPass::draw` 签名：
   ```
   // BEFORE (in truvis-gui-backend)
   fn draw(&self, render_context: &RenderContext, canvas_color_view, ...)

   // AFTER
   fn draw(&self, frame_label: FrameLabel, global_descriptor_sets: &GlobalDescriptorSets,
           bindless_manager: &BindlessManager, canvas_color_view, ...)
   ```
3. 从 gui-backend 的 `Cargo.toml` 移除 `truvis-render-graph`
4. 在 `GuiRgPass::execute` 中从 `RenderContext` 提取参数后调用 `GuiPass::draw`
5. 更新 app 中 3 个文件的 import

## 四、改动后的模块依赖层级

```
                        ┌──────────────────────────────────────────────────┐
                        │              AFTER: CLEAN LAYERS                 │
                        ├──────────────────────────────────────────────────┤
                        │                                                  │
                        │  Layer 0 (Foundation):                           │
                        │  ┌───────┐ ┌──────┐ ┌──────┐ ┌──────────────┐  │
                        │  │ utils │ │ logs │ │ path │ │shader-binding│  │
                        │  └───────┘ └──────┘ └──────┘ └──────────────┘  │
                        │                                                  │
                        │  Layer 1 (RHI):                                  │
                        │  ┌──────────────────────────────────────────┐   │
                        │  │              truvis-gfx                  │   │
                        │  └──────────────────────────────────────────┘   │
                        │                                                  │
                        │  Layer 2 (Resource Management):                  │
                        │  ┌──────────────────────────────────────────┐   │
                        │  │         truvis-render-interface           │   │
                        │  └──────────────────────────────────────────┘   │
                        │                                                  │
                        │  Layer 3 (同层，互不依赖):                        │
                        │  ┌────────────┐  ┌───────┐  ┌──────────────┐   │
                        │  │render-graph│  │ asset │  │gui-backend   │   │
                        │  │(纯 pass   │  ├───────┤  │(纯 Vulkan    │   │
                        │  │ 编排)     │  │ scene │  │ 录制)        │   │
                        │  └────────────┘  └───────┘  └──────────────┘   │
                        │                                                  │
                        │  Layer 4 (Integration):                          │
                        │  ┌──────────────────────────────────────────┐   │
                        │  │            truvis-renderer                │   │
                        │  │  (RenderContext 定义在此层)                │   │
                        │  └──────────────────────────────────────────┘   │
                        │                                                  │
                        │  Layer 5 (App Framework):                        │
                        │  ┌──────────────────────────────────────────┐   │
                        │  │             truvis-app                    │   │
                        │  │  (GuiRgPass 适配器 + render pipeline)    │   │
                        │  └──────────────────────────────────────────┘   │
                        │                                                  │
                        │  Layer 6 (Binaries):                             │
                        │  ┌──────────────────────────────────────────┐   │
                        │  │          truvis-winit-app                 │   │
                        │  └──────────────────────────────────────────┘   │
                        │                                                  │
                        └──────────────────────────────────────────────────┘
```

**改动前后 Cargo.toml 依赖对比：**

| Crate | 移除的依赖 | 原因 |
|-------|-----------|------|
| `truvis-render-graph` | `truvis-scene`, `truvis-asset` | RenderContext 搬到 renderer |
| `truvis-gui-backend` | `truvis-render-graph` | GuiRgPass 搬到 app |
| `truvis-logs` | `reqwest`, `serde`, `zip`, `toml`, `anyhow` | 幽灵依赖 |

## 五、后续可改进方向

以下问题在本次改动中明确标记为 Non-Goal，留待后续处理：

1. **`OuterApp::draw` 签名中暴露了 `GfxSemaphore`**：app 层直接依赖 gfx 层类型。可改为 renderer 层封装的 token 类型。
2. **`truvis-app` 混合了框架和示例**：render_pipeline/ 和 demo apps 可考虑拆分到独立 crate 或 truvis-winit-app。
3. **`truvis-shader-binding` 全局编译扇出**：几乎所有 crate 都依赖它。可考虑按功能域拆分（common / per-pass）。
4. **`truvis-app` 依赖扇出过大**：直接依赖了 gfx、render-interface、render-graph 等底层 crate。需要 renderer re-export 或进一步封装。
