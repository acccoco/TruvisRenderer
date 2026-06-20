# app-kit

`app-kit` 保存主体 app 与 samples 共享的 app 层组件。

## 主要职责

- `GuiPlugin`：ImGui context、输入转发、字体资源、GUI mesh 上传、debug image viewer、
  私有 Vulkan 后端和 RenderGraph pass 注入。
- `Camera` / `CameraController` / `InputManager`：示例级相机与输入状态，`Camera` 可生成
  runtime prepare 使用的 `RenderView` 快照；控制器包含右键视角、
  WASD/QE 移动、左键点击边沿输入、中键拾取 pivot 后的环视控制状态、Shift+中键拖拽场景，
  以及主应用启用的滚轮锚点移动。
- `DebugInfoOverlay` / `PipelineControlsOverlay`：UI-only overlay plugin。
- `RtPipeline`：光追示例与 Truvis 主体 app 共用的 RT pipeline glue，依赖
  `app-render-passes` 提供具体 RT 与后处理 pass，并负责 RT working target、main view target
  等 app-owned 窗口尺寸资源的 init / resize / shutdown 生命周期。
- `OfflinePipeline`：离线 ground truth pipeline glue，维护独立 sample count、Halton jitter、
  FIF 唯一累计图像和无 TLAS 时的确定黑色输出，不复用 runtime `ViewAccumState`、DLSS 或 ReSTIR 状态。

## 边界约束

- 不持有具体 app state，不提供可执行入口。
- 不保存 Triangle / ShaderToy 等 sample 专用 pass。
- 依赖 render runtime 与 render graph，定位是 app 集成层公共库，不是 engine core；GUI backend 是本 crate 的私有实现细节。
- `render_pipeline::targets` 只保存 `GfxResourceManager` image/view handle，不保存 `Gfx`、device、
  allocator 或 command allocator 引用；创建、resize 和 shutdown 必须通过对应生命周期 Ctx 显式传入
  manager、bindless manager 和 typed Gfx Ctx。
- RT working target、main view target、GBuffer 等窗口尺寸资源属于具体 pipeline/plugin owner，
  不进入 engine runtime-owned render state。resize 时先注销 bindless view，再通过 `GfxResourceManager` 释放
  manager-owned image，image view 由 manager 跟随 image 按顺序释放。
- 离线 `single_frame_image`、`accum_image`、`render_target` 同样属于 `OfflinePipeline` owner；
  `accum_image` 是跨帧历史，只有离线累计签名仍有效且 TLAS 存在时才推进 sample。
- 相机状态属于 app 层；runtime 只消费 `truvis-render-foundation` 中的 `RenderView`，不依赖
  `Camera` 或具体相机控制策略。
- GUI debug image viewer 只保存 app/pipeline 每帧注册的 image/view handle 快照和 ImGui
  texture id 映射；被选中的中间图像仍必须通过 RenderGraph 声明 fragment sampled 读取，
  由具体 pipeline owner 负责 image 生命周期、resize 和 bindless SRV/UAV 注册。
- 中键 pivot orbit、Shift+中键拖拽与滚轮锚点移动只在本层保存输入和相机控制状态；同步 raycast
  仍由具体 app 在 `after_prepare` 阶段调用 runtime 查询。左键点击 raycast 复用本层的屏幕射线生成逻辑，
  查询结果仍保存在具体 app state。未接入拖拽/滚轮 raycast 回填的 app/sample 继续使用默认
  `CameraController::update`，不会产生拖拽平移或滚轮相机移动。
