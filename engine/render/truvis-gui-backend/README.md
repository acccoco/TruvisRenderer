# truvis-gui-backend

ImGui Vulkan 后端模块，提供底层 mesh buffer 与绘制命令录制能力。

## 主要职责

- `GuiMesh`：UI 顶点/索引 buffer
- `GuiPass`：录制 ImGui 绘制命令

## 设计意图

- 本 crate 只表达底层 Vulkan backend 能力，不创建或持有 imgui context。
- `GuiPass` 接收已经准备好的 draw data、mesh buffer 和目标 image/view，负责把 ImGui draw command 录进 command buffer。
- `GuiMesh` 管理每帧 GUI 顶点/索引数据的 GPU buffer 表达，具体上传时机由上层 `GuiPlugin` 决定。
- RenderGraph adapter 属于应用集成层：`truvis_app_kit::gui_plugin::GuiPlugin` 负责把 `GuiPass` 包装成 GUI pass 并添加到 graph。

## 边界约束

- 专注 Vulkan 后端实现，不承担 RenderGraph 适配逻辑
- imgui context、字体纹理注册、mesh 上传调度和 RenderGraph 适配由 `truvis_app_kit::gui_plugin::GuiPlugin` 处理
- 本 crate 不依赖 `truvis-render-graph` 或 frame runtime
