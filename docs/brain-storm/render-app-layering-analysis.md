# RenderApp / OuterApp / Renderer / GUI 分层分析

> 维护状态（2026-04-23）：本文是历史诊断，文中的 `RenderApp` / `OuterApp` /
> `RenderContext` 主线已经分别演进为 `FrameRuntime` / `AppPlugin` / `World + RenderWorld`。
> 保留本文是为了记录早期问题来源和 GUI、surface、extract、plugin 化等演进方向。
> 当前代码状态请先看 [`README.md`](README.md) 与 [`../../ARCHITECTURE.md`](../../ARCHITECTURE.md)。

基于 [ideal_layered_architecture.md](ideal_layered_architecture.md) 中的 Bevy 风格分层理念，对当前 `truvis-winit-app` / `truvis-app` / `truvis-renderer` / `truvis-gui-backend` 的职责划分做诊断，并给出重构方向。


## 1. 当前结构速览

```
┌─────────────────────────────────────────────────────────────┐
│ WinitApp  (truvis-winit-app)                                │
│   持: RenderApp + Window                                    │
│   做: OS 事件循环、创建 Window、事件分发                      │
├─────────────────────────────────────────────────────────────┤
│ RenderApp  (truvis-app/render_app.rs)                       │
│   持: Renderer + CameraController + InputManager +           │
│       GuiHost + OuterApp                                    │
│   做: big_update() 驱动整帧（begin/update/draw/end）          │
│        构建全局 overlay UI                                   │
├─────────────────────────────────────────────────────────────┤
│ Renderer  (truvis-renderer)                                 │
│   持: RenderContext + CmdAllocator + Timer +                │
│       FifTimelineSemaphore + RenderPresent                  │
│   RenderContext 持: SceneManager + GpuScene + AssetHub +    │
│       FifBuffers + BindlessManager + GfxResourceManager +   │
│       SamplerManager + GlobalDescriptorSets + 各种 Settings │
├─────────────────────────────────────────────────────────────┤
│ OuterApp trait                                              │
│   钩子: init / update / draw / draw_ui / on_window_resized  │
└─────────────────────────────────────────────────────────────┘
```


## 2. 与 Bevy 理想架构的对照诊断

### 2.1 `Renderer` 是个混合体

`RenderContext` 的字段按理想架构应分散在三个寿命域：

| 字段 | 正确归属 | 当前位置 |
| --- | --- | --- |
| `Gfx` 单例 | **Platform** | 全局 `Gfx::get()` |
| `AssetHub` | **Platform** (跨 World 共享) | RenderContext |
| `BindlessManager` | **Platform** (descriptor heap 寿命 = device) | RenderContext |
| `GfxResourceManager` | **Platform** | RenderContext |
| `SamplerManager` | **Platform** | RenderContext |
| `RenderPresent` / `Swapchain` | **Platform** (SurfaceRegistry) | Renderer |
| `SceneManager` | **Main World** Resource | RenderContext |
| `Timer` / `delta_time_s` | **Main World** Resource | Renderer / RenderContext |
| `PipelineSettings` | **Main World** Resource | RenderContext |
| `Camera` (via CameraController) | **Main World** | RenderApp |
| `GpuScene` | **Render World** Resource | RenderContext |
| `FifBuffers` | **Render World** Resource | RenderContext |
| `per_frame_data_buffers` | **Render World** Resource | RenderContext |
| `AccumData` | **Render World** Resource | RenderContext |
| `FrameCounter` / `fif_timeline_semaphore` | **Render World** | RenderContext / Renderer |

> [!info] `Renderer` 同时扮演 Platform 句柄容器 + Main World 状态机 + Render World 资源库 + 渲染调度器。**四合一**。

症状：
- 每个字段都要同时考虑"谁能读 / 写 / 何时销毁"，代码里充满 `render_context.xxx.yyy` 的穿透访问
- `destroy()` 手动按顺序清理每个字段，易出错
- `RenderContext2<'a>` 的出现就是被迫补救——因为 `RenderContext` 混用了"写阶段资源"和"读阶段资源"

### 2.2 `RenderApp` 的上帝对象问题

`RenderApp::big_update()` 一个函数串起：

```
begin_frame → handle events (含 imgui 吞事件) → resize 处理 →
update_frame_settings → acquire_image → build_ui (含 fps/camera/settings overlay) →
compile_ui → prepare_render_data(gui) → update_scene (含 camera update + outer.update) →
before_render (gpu upload) → outer.draw → present_image → end_frame
```

这一个方法同时承担 Platform 事件泵 + Main World schedule + Extract + Render World schedule + Present。所有阶段的边界被一个大方法隐式定义，**完全无法并行**。

### 2.3 `OuterApp` trait 混合三种角色

```rust
trait OuterApp {
    fn init(&mut self, renderer, camera);                      // Platform + Main
    fn update(&mut self, renderer: &mut Renderer);             // Main World System
    fn draw_ui(&mut self, ui: &imgui::Ui);                     // Main World System (UI 构建)
    fn draw(&self, renderer, gui_draw_data, fence);            // Render World System
    fn on_window_resized(&mut self, renderer);                 // Platform event handler
}
```

- 一个 OuterApp 实例同时是 Main World 数据、Main World System、Render World System 三者，跨 World 持有字段（例：`TrianglePass` 是 GPU pipeline，本应在 Render World，却和 CPU 侧配置混在同一 struct）
- 没有 Extract 阶段：`draw()` 直接访问 `renderer` 全部，意味着 GPU 侧直接读 CPU 侧数据，无法实现双 World 并行
- `update` 传的是 `&mut Renderer`，等价于 Bevy 里一个 System 拿到整个 `&mut World`——违反最小权限

### 2.4 `Gfx::get()` 全局单例

违反健康度清单中的「没有 `::get()` 形式的全局访问」。`Gfx::get().gfx_queue().begin_label(...)` 这种调用遍布各处：

- 无法 mock，测试困难
- 隐藏依赖，函数签名看不出它用了 GPU device

### 2.5 Viewport 概念缺失

当前 `Camera.asp` 直接从 swapchain extent 读：

```rust
// render_app.rs: update_scene
let frame_extent = self.renderer.render_context.frame_settings.frame_extent;
self.camera_controller.update(input_state, vec2(frame_extent), ...);
```

Main World 里的逻辑对象直接耦合 swapchain 大小。Bevy 做法：Main World 只有 `Viewport { camera_entity, target_window: WindowId, render_scale }`，Extract 阶段再翻译成 `ExtractedViewport { view/proj, surface, size }`。

代价：当前架构不可能做 headless 渲染、离屏截图、多视口（主窗口 + 材质预览窗）都要大改。

### 2.6 Swapchain 归属错位

`RenderPresent` 当前住在 `Renderer` 里，包含 swapchain + gui_backend + present 队列。

- Swapchain 按理属于 **Platform 的 SurfaceRegistry**（生命周期跟 Window 走，resize 由 OS 事件触发）
- 但它被塞进 Renderer，导致 Renderer 既是 GPU 渲染器又是窗口管理员
- Resize 链路：WinitApp → RenderApp.handle_event → big_update 里查 `renderer.need_resize()` → `recreate_swapchain()` → 通知 outer_app。没有事件总线，属于**轮询而非响应**

### 2.7 Resource vs Subsystem 未区分

`Renderer` 既有数据（`render_context`、`cmd_allocator`）又有行为（`begin_frame()`、`before_render()`、`update_gpu_scene()`、`update_perframe_descriptor_set()`）。按 Bevy 的分法，这些行为应该拆成独立的 System，读写声明明确的 Resource。


## 3. 理想的重分层

```
┌──────────────────────────────────────────────────────────────────┐
│ Platform (WinitApp 构造时创建，注入给下层)                         │
│   Gfx (改为对象，非单例)                                            │
│   WindowManager                                                   │
│   SurfaceRegistry  (← 从 RenderPresent 抽出 swapchain)             │
│   AssetHub / AssetServer                                          │
│   BindlessManager  (← 从 RenderContext 抽出)                      │
│   GfxResourceManager                                              │
│   SamplerManager                                                  │
│   GlobalDescriptorSets                                            │
│   ImguiTextureRegistry  (TextureId → GfxImageViewHandle)          │
├──────────────────────────────────────────────────────────────────┤
│ AppShell (RenderApp 瘦身)                                          │
│   持: Platform + MainWorld + RenderWorld                          │
│   做: tick 驱动(main→extract→render)、事件分发、窗口生命周期         │
│   不持: 任何业务状态，不构建 overlay UI                             │
├──────────────────────────────────────────────────────────────────┤
│ MainWorld                      │ RenderWorld                      │
│  Resources:                    │  Resources:                      │
│    Time                        │    GpuScene                      │
│    InputState                  │    FifBuffers                    │
│    SceneManager                │    PerFrameDataBuffers           │
│    Viewports                   │    AccumData                     │
│    PipelineSettings            │    FrameCounter / Timeline sem   │
│    Cameras (ECS)               │    ExtractedViewports            │
│    ImguiContext + UI tree      │    ExtractedImguiDrawData        │
│                                │    ExtractedSceneDelta           │
│  Systems:                      │    PipelineCache                 │
│    InputSystem                 │  Systems (Schedule):              │
│    CameraControlSystem         │    1. Extract*                   │
│    ImguiBuildSystem            │    2. AcquireImage                │
│    OuterApp.update             │    3. PrepareGpuScene             │
│    OuterApp.build_ui           │    4. ExecuteRenderGraph          │
│                                │    5. Submit + Present           │
└────────────────────────────────┴──────────────────────────────────┘
          │                                     ▲
          └────── Extract (单向、每帧一次) ──────┘
```

代码结构变化：

| 当前 | 目标 |
| --- | --- |
| `Renderer` | 拆：`Platform` 对象 + `MainWorld` + `RenderWorld` |
| `RenderContext` | 拆：按字段迁移到上面三层 |
| `RenderContext2<'a>` | 删除（被 `&RenderWorld` 自然替代） |
| `Gfx::get()` | `Gfx` 对象通过构造注入 |
| `RenderApp::big_update` | 三段式 schedule：`main.tick(); extract(main, render); render.tick()` |
| `OuterApp` (单 trait) | 拆成 `MainPlugin` (update/build_ui) + `RenderPlugin` (draw) + `ExtractSystem` |
| `RenderPresent` | 拆：`Swapchain` 归 `SurfaceRegistry` (Platform)，`GuiBackend` 独立 |


## 4. GUI 拆分分析

### 4.1 当前拆法

| 组件 | 所在 crate | 职责 | 归属层（按理想） |
| --- | --- | --- | --- |
| `GuiHost` | truvis-app | imgui::Context、new_frame、font 加载、input 转译 | **Main World** (UI 逻辑) |
| `GuiBackend` | truvis-gui-backend | mesh FIF 缓冲、font 注册、prepare_render_data、tex_map | **Render World** |
| `GuiPass` | truvis-gui-backend | pipeline、`draw()` 录制 | **Render World** |
| `GuiRgPass` | truvis-app | `RgPass` trait 实现，包装 GuiPass 进 render graph | **Render World** |
| `GuiMesh` | truvis-gui-backend | 顶点 / 索引缓冲 per-FIF | **Render World** |

### 4.2 诊断

**问题 1：双源常量**

[gui_backend.rs](../../engine/crates/truvis-gui-backend/src/gui_backend.rs) 注释直接承认：

```rust
// TODO 这个东西和 GuiHost 的重复了
const FONT_TEXTURE_ID: usize = 0;
const RENDER_IMAGE_ID: usize = 1;
```

`FONT_TEXTURE_ID` 在 Main 和 Render 两侧各定义一份——这正是**缺少 Extract 契约**的症状。理想做法：Platform 的 `ImguiTextureRegistry` 唯一持有，双方按 `TextureId` 查询。

**问题 2：GuiBackend 藏在 `RenderPresent` 里**

```rust
self.renderer.render_present.as_mut().unwrap().gui_backend.register_font(...)
self.renderer.render_present.as_mut().unwrap().gui_backend.prepare_render_data(...)
```

GUI backend 跟 swapchain 没有必然关系（GUI 也可能画到 offscreen target），放进 `RenderPresent` 只是"凑位置"。应独立成 `RenderWorld` 的 Resource。

**问题 3：GuiHost 在 `RenderApp` 而 GuiBackend 在 `Renderer`**

构建 UI 的链路跨 5 层对象：

```
RenderApp.build_ui 
  → GuiHost.new_frame (在 RenderApp 方法里直接访问 camera/renderer.settings 构建 overlay)
  → GuiHost.compile_ui
  → RenderPresent.gui_backend.prepare_render_data
  → GuiRgPass.execute
    → GuiPass.draw
```

**问题 4：overlay UI 写死在 `RenderApp::build_ui`**

fps / camera info / pipeline settings 面板全硬编码在 RenderApp 里，和 OuterApp 的 `draw_ui` 并列但优先级不清。应用无法禁用或替换这些 overlay。

**问题 5：imgui_ctx 与引擎强耦合**

`truvis-gui-backend` crate 依赖 `truvis-render-interface`，`GuiHost` 又在 `truvis-app`。想替换 GUI（egui、纯代码覆盖）需改多个 crate。

### 4.3 理想拆分

```
┌──────────────────────────────────────────────────────────────┐
│ Platform                                                     │
│   ImguiTextureRegistry                                       │
│     • 唯一定义 TextureId 语义 (FONT=0, RENDER=1, ...)        │
│     • 维护 TextureId ↔ GfxImageViewHandle                    │
│     • 跨 World 稳定（和 BindlessManager 一样寿命 = device）   │
├──────────────────────────────────────────────────────────────┤
│ Main World                                                   │
│   ImguiUi Resource                                           │
│     • imgui::Context, io, font config                        │
│     • new_frame / compile_ui                                 │
│   ImguiInputSystem                                           │
│     • InputEvent → imgui io                                  │
│   UiBuildSystem (可多个)                                      │
│     • OverlayUiSystem (fps/camera，可禁用)                    │
│     • PipelineSettingsUiSystem                               │
│     • OuterAppUiSystem (调用 plugin.build_ui)                │
│   ⚠ 不依赖 vulkan / truvis-gfx                              │
├──────────────────────────────────────────────────────────────┤
│ Extract Phase                                                │
│   extract_imgui_draw_data:                                   │
│     拷贝 DrawData → ExtractedImguiDrawData (Render Resource) │
│     （或用双缓冲零拷贝交换）                                   │
├──────────────────────────────────────────────────────────────┤
│ Render World                                                 │
│   ImguiGpuResources Resource                                 │
│     • GuiMesh [FIF]                                          │
│     • font_image_view_handle                                 │
│   ImguiPreparePass System                                    │
│     • 从 ExtractedImguiDrawData 上传 mesh                    │
│   ImguiPass (RgPass)                                         │
│     • 纯 GPU 录制                                            │
└──────────────────────────────────────────────────────────────┘
```

对应 crate 依赖图：

```
truvis-gui-core   (纯 imgui 封装、Context、InputAdapter)      ← Main 侧
    ↑
    └─ truvis-gui-backend  (GPU mesh、Pass、Prepare)          ← Render 侧
            ↑
            └─ 业务层 (OuterApp plugin)
```

### 4.4 GUI 拆分改动要点

| 当前 | 目标 |
| --- | --- |
| `GuiBackend` 在 `RenderPresent` 内 | 独立成 Render World Resource |
| `FONT_TEXTURE_ID` 两处定义 | Platform 单点定义，其他侧 import |
| `RenderApp::build_ui` 硬编码 overlay | 拆成可注册的 `UiBuildSystem`，应用按需启用 |
| `GuiHost.handle_event` 在 big_update 里被调 | 独立成 `ImguiInputSystem`，订阅 InputEvent |
| `prepare_render_data` 在 extract/record 之间含糊定位 | 明确为 Extract 阶段的产物（上传 mesh） |
| `GuiRgPass` 夹在 truvis-app 里引 gui_backend | 迁到 truvis-gui-backend |


## 5. 重构优先级建议

按收益 / 风险排序：

| 优先级 | 改动 | 收益 | 风险 |
| --- | --- | --- | --- |
| P0 | 把 `RenderPresent` / `Swapchain` 从 `Renderer` 分离，独立成 `SurfaceRegistry` | 解耦窗口 / 渲染；未来多窗口 / headless | 中（resize 链路要重做） |
| P0 | GUI 常量 / TextureId 单点化，`GuiBackend` 脱离 `RenderPresent` | 消除双源 bug；模块边界清晰 | 低 |
| P1 | 引入 `Viewport` 语义层，Camera 解耦 swapchain | 支持多视口 / 无头；为 Extract 铺路 | 中 |
| P1 | 把 `RenderContext` 按三层拆分；`Gfx` 去单例化（构造注入） | 依赖显式；可测试 | 高（改动面大） |
| P2 | 引入 Extract Phase，`OuterApp` 拆 `MainPlugin` + `RenderPlugin` | Main / Render 可并行；符合 Bevy 范式 | 高 |
| P2 | UI overlay 改为可注册 `UiBuildSystem` 列表 | 应用可定制；`RenderApp` 瘦身 | 低 |


## 6. 核心问题一句话总结

> `Renderer` 是 Platform / Main / Render 三层资源的合订本，`RenderApp` 是事件循环 / MainSchedule / Extract / RenderSchedule 四阶段的合订本，`OuterApp` 是 Main System / Render System / Plugin 三角色的合订本。GUI 则被撕裂在 `RenderApp`（Host）、`RenderPresent`（Backend）、`truvis-app`（RgPass）三处，且用双份常量维持两端对齐。
> 
> 重构方向：把这些"合订本"按 Platform / MainWorld / RenderWorld 三个寿命域拆开，用**构造注入**替代 `::get()`，用**显式 Extract 阶段**替代直接跨域访问。
