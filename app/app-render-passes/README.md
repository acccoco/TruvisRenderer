# app-render-passes

`app-render-passes` 存放 Truvis 主体 app 与 samples 共享的具体 render pass 实现，
例如 real-time ray tracing、accumulation、denoise、SDR、blit、resolve 和 Phong shading。

## 主要职责

- 提供具体 GPU pass 的 pipeline、descriptor、dispatch/draw 逻辑。
- 提供可接入 `truvis-render-graph` 的 pass adapter。
- 使用 `RenderPassRecordCtx` 读取 GPU frame state、shader-visible bindings 和资源 manager。
- 在需要场景数据的 pass 中通过 `RenderSceneView` 读取 scene buffer / TLAS / raster draw 能力，不在 render phase 访问 `World` 或重新 prepare scene。

## 边界约束

- 本 crate 不负责 App 级 pass 顺序、GUI overlay 顺序或 demo pipeline 编排。
- 本 crate 不持有 `RenderRuntime`，也不依赖 frame runtime 或 App hooks。
- runtime-owned 同步 raycast pipeline 不在本 crate 中；它是 `truvis-render-runtime` 的私有实现细节。
- `GuiPass` 不在本 crate 中；GUI Vulkan 后端是 `app/app-kit` 的私有实现细节，GUI RenderGraph 集成属于 `GuiPlugin`。

## 设计意图

本 crate 只表达“如何录制 Truvis app 复用的具体 GPU 效果”。具体 App 在 `RenderAppHooks::render`
中创建 `RenderGraphBuilder`，再按业务顺序组合 `app/app-kit` 中的 render pipeline glue、
post-process pass 和 GUI pass。这样新增 demo 或 pipeline 时优先复用 pass 实现，而不把
App 级编排逻辑下沉到 engine core。
