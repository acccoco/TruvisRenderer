# renderer-render-ui

`renderer-render-ui` 是渲染设置与 ImGui 之间的共享集成层，不拥有 GPU 资源、渲染 subsystem 或具体 Renderer 布局。

## 公共接口

- `build_render_mode_section` / `build_dlss_section_for_mode`：供同时持有 realtime/offline subsystem 的 Renderer 组合模式与 DLSS 控件。
- `build_sampling_section` / `build_sky_section` / `build_tone_mapping_section`：分别编辑 NEE 与 Offline dispatch 数、天空以及显示映射，共享配置不按渲染模式复制。
- `build_realtime_settings_section` / `build_debug_channel_section`：组合 realtime-only ReSTIR/SHARC，并按模式选择 debug channel；Offline 候选复用 `OfflineRenderSettings::supports_debug_channel`。
- Offline 模式禁用 DLSS、ReSTIR 与 SHARC，并显示原因；窗口、tab 顺序和可见性仍由具体 Renderer 决定。
- 控件只响应用户操作，不承担配置归一化；Renderer 必须在固定 update 路径维护配置，避免合法性依赖页面可见性。

## 边界约束

- 依赖 `renderer-imgui`、`renderer-rendering`、`renderer-kit` 与 Engine 的 `DlssOptions`，不直接依赖 `renderer-render-passes`。
- `SdrPostProcessSettings` 只通过 `renderer-rendering::shared` 获取；UI 不持有 image、pipeline、descriptor 或 RenderGraph。
- Triangle / ShaderToy 不依赖本 crate，也不显示 render mode、DLSS 或 path tracing controls。
