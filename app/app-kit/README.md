# app-kit

`app-kit` 保存主体 app 与 samples 共享的 app 层组件。

## 主要职责

- `GuiPlugin`：ImGui context、输入转发、字体资源、GUI mesh 上传和 RenderGraph pass 注入。
- `CameraController` / `InputManager`：示例级相机与输入状态。
- `DebugInfoOverlay` / `PipelineControlsOverlay`：UI-only overlay plugin。
- `RtPipeline`：光追示例与 Truvis 主体 app 共用的 compute/present graph glue。

## 边界约束

- 不持有具体 app state，不提供可执行入口。
- 不保存 Triangle / ShaderToy 等 sample 专用 pass。
- 依赖 render runtime、render graph 与 GUI backend，定位是 app 集成层公共库，不是 engine core。
