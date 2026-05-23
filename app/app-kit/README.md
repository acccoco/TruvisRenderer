# app-kit

`app-kit` 保存主体 app 与 samples 共享的 app 层组件。

## 主要职责

- `GuiPlugin`：ImGui context、输入转发、字体资源、GUI mesh 上传和 RenderGraph pass 注入。
- `CameraController` / `InputManager`：示例级相机与输入状态，包含右键视角、
  WASD/QE 移动、左键点击边沿输入、中键拾取 pivot 后的环视控制状态，以及主应用启用的滚轮锚点移动。
- `DebugInfoOverlay` / `PipelineControlsOverlay`：UI-only overlay plugin。
- `RtPipeline`：光追示例与 Truvis 主体 app 共用的 compute/present graph glue。

## 边界约束

- 不持有具体 app state，不提供可执行入口。
- 不保存 Triangle / ShaderToy 等 sample 专用 pass。
- 依赖 render runtime、render graph 与 GUI backend，定位是 app 集成层公共库，不是 engine core。
- 中键 pivot orbit 与滚轮锚点移动只在本层保存输入和相机控制状态；同步 raycast 仍由具体 app 在
  `after_prepare` 阶段调用 runtime 查询。左键点击 raycast 复用本层的屏幕射线生成逻辑，
  查询结果仍保存在具体 app state。未接入滚轮 raycast 回填的 app/sample 继续使用默认
  `CameraController::update`，不会产生滚轮相机移动。
