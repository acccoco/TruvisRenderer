# renderer-render-ui

`renderer-render-ui` 是渲染设置与 ImGui 之间的共享集成层，不拥有 GPU 资源、渲染 subsystem 或具体 Renderer 布局。

## 公共接口

- `RenderControlsOverlay::build_realtime_window`：为 Cornell 等只持有 realtime subsystem 的 Renderer 绘制 DLSS 和 path tracing 设置，不构造不存在的 offline mode 或 owner。
- `build_render_mode_section` / `build_dlss_section_for_mode`：供同时持有 realtime/offline subsystem 的 Renderer 组合模式与 DLSS 控件。
- `build_realtime_section` / `build_offline_section` / `build_mode_specific_sections`：组合共享 sampling、tone mapping 以及 realtime-only ReSTIR/SHARC 设置。
- Offline 模式保留既有禁用状态和 sample count；窗口、section 顺序和可见性仍由具体 Renderer 决定。

## 边界约束

- 依赖 `renderer-imgui`、`renderer-rendering`、`renderer-kit` 与 Engine 的 `DlssOptions`，不直接依赖 `renderer-render-passes`。
- `SdrToneMappingSettings` 只通过 `renderer-rendering::shared` 获取；UI 不持有 image、pipeline、descriptor 或 RenderGraph。
- Triangle / ShaderToy 不依赖本 crate，也不显示 render mode、DLSS 或 path tracing controls。
