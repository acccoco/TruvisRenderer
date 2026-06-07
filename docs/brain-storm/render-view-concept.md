# Render View 概念

> 状态：活跃方向，更新于 2026-05-24。当前代码已引入 `RenderView` 作为 prepare 边界。

当前渲染路径已经存在一个显式 main view：App 持有 camera，并把它转换为
`RenderRuntime::prepare(render_view)` 需要的 `RenderView` 快照。runtime 再把 view、
frame extent、accum state、per-frame uniform 和 FIF resources 组合成本帧渲染输入。
本文只记录后续轻量显式化方向，不要求一次性引入重型 view family。

## 目标

第一阶段只命名并约束现有 main view：

- 让 App 描述“这次从哪里看、画到哪里、使用什么设置”。
- 让 runtime 生成 shader 可读的 prepared view 数据。
- 把 per-frame 中与 camera / projection / resolution 相关的字段逐步改名为 per-view 语义。
- 为 editor viewport、shadow、reflection、probe 等多 view 场景预留扩展点。

## 最小概念

```rust
pub struct ViewDesc {
    pub id: ViewId,
    pub name: String,
    pub camera: CameraSnapshot,
    pub extent: vk::Extent2D,
    pub target: ViewTarget,
    pub settings: ViewSettings,
}

pub struct PreparedView {
    pub id: ViewId,
    pub view: glam::Mat4,
    pub projection: glam::Mat4,
    pub inv_view: glam::Mat4,
    pub inv_projection: glam::Mat4,
    pub camera_pos: glam::Vec3,
    pub camera_forward: glam::Vec3,
    pub resolution: glam::Vec2,
    pub accum_frames: u32,
}
```

第一版可以只支持 `ViewTarget::MainFif`，也可以继续复用现有 per-frame uniform buffer，
但 Rust 侧命名和数据流应开始表达 view。

## 放置建议

- 纯数据类型优先放在 render foundation 或 runtime 公共 API 中，避免依赖具体 App state。
- camera 快照应是 POD 风格数据，不要求 View 类型直接依赖 App 持有的 camera controller。
- `PreparedView` 由 runtime 在 prepare 阶段生成，并写入当前 FIF 对应的 uniform / descriptor。
- `RenderSceneView` 继续表达 scene 数据；View 不应拥有 scene、runtime GPU owner、manager 或 RenderGraph。

## 非目标

- 不做 UE 风格重型 ViewFamily。
- 不在第一版支持多窗口、多 surface、shadow atlas 或 per-view culling。
- 不把 View 做成新的大上下文；pass 仍通过明确参数、`RenderPassRecordCtx` 和 `RenderSceneView` 读取所需能力。
- 不为了 View 立即修改 shader binding 布局，除非 per-view 命名必须同步更新。

## 演进步骤

1. 在 runtime prepare 内部构建 main `ViewDesc` / `PreparedView`，行为保持不变。
2. 已完成：将 prepare 的 app 相机参数收窄为 `RenderRuntime::prepare(render_view)`。
3. 把 per-frame uniform 写入 helper 改名为 prepared-view upload，保留当前 GPU layout。
4. 让 App render hook 从 render ctx 读取当前 prepared main view，而不是自己推导 extent / camera 语义。
5. 多 view 需求出现后再引入 `ViewStore`、per-view temporal state 和 view target 抽象。
