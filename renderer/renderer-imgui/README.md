# renderer-imgui

`renderer-imgui` 拥有 Renderer 层可复用的 `ImGuiSubsystem`、私有 Vulkan backend 和不依赖渲染模式的诊断控件。

## 主要职责

- `ImGuiSubsystem`：ImGui context、输入适配、闭包式 `build_frame`、字体 image/view、per-FIF mesh、draw data 上传和 GUI RenderGraph pass。
- `DebugInfoOverlay` / `FrameStatsOverlayData`：使用 ImGui 内建滚动估算的 FPS，以及相机、窗口尺寸和稳定视图 update 次数等通用诊断显示；提供轻量 FPS HUD、完整 HUD 和可嵌入 section，共用不捕获输入的 HUD 窗口。稳定次数由 Renderer 提供，不表示 GPU 提交或累计样本数。
- `DebugImageSelectorView`：把 `renderer-kit::DebugImageSelection` 渲染为独立窗口或可嵌入 section；视图不保存 GPU image/view，候选归一化由 Renderer 每帧执行，不依赖控件是否可见。
- `backend`：GUI graphics pipeline、mesh 和 vertex layout；底层 pass 与 draw data 不作为公共接口暴露。

顶点上传直接使用 ImGui 的 `DrawVert`，Vulkan stride、attribute offset 和容量计算也从该类型取得，
不维护私有顶点镜像。`pos` / `uv` 是两个 float，`col` 是四个字节，按 `R8G8B8A8_UNORM` 输入 shader。
上传点用编译期 size/offset 断言证明该第三方类型无 padding，再借用字节传入 Gfx 的受检写入接口。

## 生命周期与依赖

- 具体 Renderer 静态持有 `ImGuiSubsystem`，显式调用 `on_input`、`build_frame`、`prepare_render_data` 和 `contribute_passes`。
- 字体和 mesh 在 RenderThread 的 `init` / `shutdown` 阶段创建和显式释放；GUI pass 顺序由 Renderer 决定。
- resize 更新 display size，并在 present format 改变时重建 GUI pipeline；格式未变时复用，字体、mesh 和 ImGui context 不随此格式重建。
- 依赖 `renderer-kit`、Engine runtime/render graph 和统一的 `truvis-renderer-shader-binding`，不依赖 `renderer-rendering` 或 `renderer-render-passes`。
- GUI shader 仍位于 `renderer/shader/entry/renderer/ui/imgui.slang`，ABI namespace 保持 `renderer::kit::ui_imgui`；Rust crate 拆分不改变 shader package 或生成 binding。
