# truvis-gui-backend

ImGui Vulkan 后端模块，提供底层 mesh buffer 与绘制命令录制能力。

## 主要职责

- `GuiMesh`：UI 顶点/索引 buffer
- `GuiPass`：录制 ImGui 绘制命令

## 边界约束

- 专注 Vulkan 后端实现，不承担 RenderGraph 适配逻辑
- imgui context、字体纹理注册、mesh 上传调度和 RenderGraph 适配由 `truvis-app::gui_plugin::GuiPlugin` 处理
- 本 crate 不依赖 `truvis-render-graph` 或 frame runtime
