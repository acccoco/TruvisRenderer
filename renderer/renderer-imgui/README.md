# app-imgui

`app-imgui` 拥有 App 层可复用的 `ImGuiSubsystem`、私有 Vulkan backend 和不依赖渲染模式的诊断控件。

## 主要职责

- `ImGuiSubsystem`：ImGui context、输入适配、闭包式 `build_frame`、字体 image/view、per-FIF mesh、draw data 上传和 GUI RenderGraph pass。
- `DebugInfoOverlay` / `FrameStatsOverlayData`：使用 ImGui 内建滚动估算的 FPS，以及相机、窗口尺寸和累计帧数等通用诊断显示。
- `DebugImageSelectorView`：把 `app-kit::DebugImageSelection` 渲染为独立窗口或可嵌入 section；视图不保存 GPU image/view。
- `backend`：GUI graphics pipeline、mesh 和 vertex layout；底层 pass 与 draw data 不作为公共接口暴露。

## 生命周期与依赖

- 具体 Renderer 静态持有 `ImGuiSubsystem`，显式调用 `on_input`、`build_frame`、`prepare_render_data` 和 `contribute_passes`。
- 字体和 mesh 在 RenderThread 的 `init` / `shutdown` 阶段创建和显式释放；GUI pass 顺序由 Renderer 决定。
- 依赖 `app-kit`、Engine runtime/render graph 和统一的 `truvis-app-shader-binding`，不依赖 `app-rendering` 或 `app-render-passes`。
- GUI shader 仍位于 `app/shader/entry/app/ui/imgui.slang`，ABI namespace 保持 `app::kit::ui_imgui`；Rust crate 拆分不改变 shader package 或生成 binding。
