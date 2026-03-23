# ImGUI 系统

ImGUI 和多个系统都有关联，因此被拆分成了这几个模块：

- 平台层：负责处理窗口事件
- Render 层：负责构建 command list，维护 imgui 的渲染资源
- 应用层：负责 imgui 的 UI 构建