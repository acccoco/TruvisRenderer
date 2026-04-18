# truvis-gui-backend

ImGui Vulkan 后端模块，负责将 UI DrawData 转换为 GPU 绘制命令。

## 主要职责

- 字体纹理与相关资源初始化
- UI 顶点/索引数据上传
- 录制 ImGui 绘制命令

## 边界约束

- 专注 Vulkan 后端实现，不承担 RenderGraph 适配逻辑
- 图编排层适配由上层模块处理
