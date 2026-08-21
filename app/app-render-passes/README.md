# app-render-passes

`app-render-passes` 存放 Truvis 主体 app 与 samples 共享的具体 render pass 实现，
例如 real-time / offline ray tracing、accumulation、SDR、image clear、resolve、
coordinate gizmo、selection outline 和 Phong shading。

## Shader 所有权

- shader 源码统一位于 `app/shader/`；`abi/app/render_passes/` 保存
  `app::render_passes::*` CPU/GPU 契约，`lib/app/` 保存依赖 pass ABI/resource 的算法，
  `entry/app/` 保存最终入口。
- `app/shader/truvis-app-shader-binding` 统一生成全部 App ABI，只 allowlist `app::*`，并依赖
  Engine canonical `truvis-shader-binding`。
- 本 crate 使用 `app` package，产物位于 `build/shader/app/`；Engine ABI/lib 通过
  `depends_on = ["engine"]` 单向引用。
- 完整目录、include roots 与增量边界见 `app/shader/README.md`。

## 主要职责

- 提供具体 GPU pass 的 pipeline、descriptor、dispatch/draw 逻辑。
- 提供可接入 `truvis-render-graph` 的 pass adapter。
- 使用 `RenderPassRecordCtx` 读取 GPU frame state、shader-visible bindings 和资源 manager。
- 在需要场景数据的 pass 中通过 `RenderSceneView` 读取 scene buffer / TLAS / raster draw 能力，不在 render phase 访问 `World` 或重新 prepare scene。
- `SelectionOutlinePass` 只负责录制 R8 mask 光栅化与 present composite；mask image 生命周期、selection
  状态和 pass 插入顺序属于具体 App。
- `CoordinateGizmoPass` 只负责在 present image 右下角叠加当前相机朝向下的三轴 gizmo；它不持有几何 buffer
  或中间 image，pass 插入顺序属于具体 App。
- `ImageClearPass` 只负责通过 pass-local storage image descriptor 把目标写成确定颜色；具体 pipeline 必须通过
  RenderGraph 声明目标图像写状态，并决定何时清理历史。
- `ResolvePass` 在同一个 dynamic rendering scope 内先绘制全屏 main image，再按需重新绑定 sampled image
  descriptor 绘制右侧 debug thumbnail；color attachment 只执行一次 `CLEAR`，不会为 thumbnail 新建 pipeline 或 rendering scope。
- 固定管线 image 由具体 pass 通过 set 3 的 typed push descriptor 引用。storage image 固定使用 `GENERAL`，
  sampled image 固定使用 `SHADER_READ_ONLY_OPTIMAL`；descriptor 中的 layout 必须与该 pass 的 RenderGraph 声明一致。

## 边界约束

- 本 crate 不负责 App 级 pass 顺序、GUI overlay 顺序或 demo pipeline 编排。
- 本 crate 不持有 `RenderRuntime`，也不依赖 frame runtime 或 App hooks。
- runtime-owned 同步 raycast pipeline 不在本 crate 中；它是 `truvis-render-runtime` 的私有实现细节。
- `GuiPass` 不在本 crate 中；GUI Vulkan 后端是 `app/app-kit` 的私有实现细节，GUI RenderGraph 集成属于 `GuiPlugin`。
- shader 源码可以引用 Engine ABI/lib；Engine shader、binding 与构建配置禁止引用本模块的 shader root 或 binding crate。
- pass-local descriptor 只描述本次 draw/dispatch 使用哪个 image view，不拥有 image/view，也不替代 RenderGraph 的
  layout transition、访问同步或 pipeline owner 的 GPU-safe 释放责任。

## 设计意图

本 crate 只表达“如何录制 Truvis app 复用的具体 GPU 效果”。具体 App 在 `RenderApp::render`
中创建 `RenderGraphBuilder`，再按业务顺序组合 `app/app-kit` 中的 render pipeline glue、
post-process pass 和 GUI pass。这样新增 demo 或 pipeline 时优先复用 pass 实现，而不把
App 级编排逻辑下沉到 engine core。
