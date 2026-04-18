# Plugin 在渲染器中的角色，以及 ImGui / Winit / 多管线集成建议

本文参考 Bevy 风格（尤其是 `App + Plugin + Render SubApp + Extract/Prepare/Queue/Render`）来回答四个问题：

1. Plugin 在渲染器里是什么角色，如何与整体互动
2. ImGui 应该怎样集成
3. Winit 应该怎样集成
4. 光栅 / 光追 / Shadertoy 多套渲染管线如何集成且可替换


## 1. Plugin 在渲染器里的角色

### 1.1 Plugin 不是“代码分组”，而是“能力装配单元”

一个渲染相关 Plugin 通常做四类事：

- 注册数据：`Resource`、`Component`、配置、事件
- 注册系统：把系统挂到明确的调度阶段（如 `Extract`、`Prepare`、`Queue`）
- 注册图节点：向 RenderGraph 注入 pass/node 和依赖边
- 声明依赖：要求在某些 Plugin 前后装配，或依赖某些能力已存在

因此，Plugin 的核心价值是：**把“这个功能需要哪些数据 + 在什么时机执行 + 对谁有依赖”声明清楚**。


### 1.2 与渲染器的互动方式（推荐生命周期）

可以用类似 Bevy 的双世界模型：

- Main/App World：游戏逻辑、输入、场景、相机、UI 逻辑
- Render World：GPU 资源、渲染队列、RenderGraph 执行

每帧通过分阶段交互：

1. `Extract`：从 Main World 抽取渲染快照（只读拷贝/双缓冲交换）
2. `Prepare`：创建或更新 GPU 资源（buffer/texture/acceleration structure）
3. `Queue`：排序、批处理、写入 draw/dispatch 命令
4. `Render`：执行 render graph，最后 present

Plugin 会把自己的系统挂进这些阶段。也就是说，Plugin 并不“直接控制渲染器主循环”，而是“接入渲染器生命周期的钩子点”。


### 1.3 推荐分层位置

建议按寿命域分层：

- Platform 层：Window、Surface、Device、Swapchain 等平台相关对象
- Main World：CPU 侧逻辑与可编辑状态
- Render World：GPU 侧资源与渲染执行状态

Plugin 可以是：

- MainPlugin（逻辑/输入/UI 构建）
- RenderPlugin（GPU prepare/queue/render）
- 或一个组合 Plugin（内部再注册上述两个子插件）


## 2. ImGui 该以什么形式集成

### 2.1 定位：独立“UI Overlay Render Feature Plugin”

ImGui 不应散落在渲染器主循环各处，建议作为一个独立插件：

- `ImguiInputBridgeSystem`：把窗口输入事件写入 `imgui::Io`
- `ImguiBuildUiSystem`：`new_frame()` 后构建 UI
- `ImguiExtractSystem`：提取 `DrawData` 到 Render World
- `ImguiPrepareSystem`：上传动态顶点/索引数据到 GPU
- `ImguiRenderPassNode`：在最终阶段绘制 UI（通常主场景和后处理之后，present 之前）

### 2.2 关键边界

- 输入抢占：当 `want_capture_mouse/keyboard` 为 true 时，阻止事件继续驱动相机等逻辑
- 纹理注册：提供统一 `TextureId` 注册表，避免 UI 侧和渲染侧重复定义常量
- 解耦：ImGui 插件不拥有 swapchain，不耦合某条业务渲染管线

### 2.3 渲染顺序建议

典型顺序：

`Scene Passes -> PostProcess Passes -> ImGui Pass -> Present`

这样 UI 作为 overlay，天然具备“后画覆盖”语义。


## 3. Winit 该以什么形式集成

### 3.1 定位：Platform/Window Backend Plugin

Winit 应视作“平台适配层”，不是渲染管线的一部分。建议职责：

- 驱动事件循环（event loop）
- 创建/销毁窗口，处理 resize、DPI、焦点等平台事件
- 暴露窗口句柄与 surface 生命周期接口
- 把 OS 事件转换为引擎内部事件流（供 Main World 系统消费）

### 3.2 与渲染器的关系

推荐依赖方向：

- `WinitPlugin -> 提供 Window/Surface 抽象`
- `RendererPlugin -> 依赖 WindowSurfaceProvider 抽象`

避免 `Renderer` 直接依赖 winit 细节。这样后续替换 SDL/GLFW/headless backend 成本更低。


## 4. 多套渲染管线如何集成且可替换

目标是同时支持：

- 光栅（Raster）
- 光追（Ray Tracing）
- Shadertoy 风格全屏程序化管线

并能在运行时或配置层面切换。


### 4.1 统一管线契约（trait/interface）

先定义统一接口，再让每条管线插件实现：

```rust
trait RenderPipelineFeature {
    fn id(&self) -> PipelineId;
    fn supports(&self, caps: &GpuCapabilities) -> bool;
    fn extract(&mut self, main: &MainWorld, render: &mut RenderWorld);
    fn prepare(&mut self, render: &mut RenderWorld);
    fn queue(&mut self, render: &mut RenderWorld);
    fn register_graph(&self, graph: &mut RenderGraph);
}
```

这保证外层调度与切换逻辑统一，不需要在主循环里写巨型 `if/else`。


### 4.2 每条管线独立插件化

建议拆为：

- `RasterPipelinePlugin`
- `RayTracingPipelinePlugin`
- `ShaderToyPipelinePlugin`

每个插件管理自己的：

- pipeline layout / descriptor set / shader
- 特定资源（如 RT 的 BLAS/TLAS、历史缓冲、denoise 资源）
- graph 节点和依赖关系

共享的数据语义（camera/light/time/material 参数）由 Extract 层统一翻译，避免每条管线各自发明一套。


### 4.3 PipelineManager：统一选择与回退

增加 `PipelineManager` 作为策略层：

- 支持全局选择 active pipeline
- 或按 View/Camera 选择 pipeline（更灵活，便于对比和调试）
- 切换时检查 `supports(caps)`，不满足则自动回退（如 RT -> Raster）
- 负责切换生命周期（初始化、热切换、失活清理）


### 4.4 Shadertoy 的合理定位

Shadertoy 建议作为“全屏 pass 特化管线插件”：

- 输入统一约定（`iTime`、`iResolution`、`iMouse`、`iChannel0..3`）
- 可挂到主渲染目标或离屏目标
- 作为独立 PipelineFeature 参与同样的选择与调度

不要把 Shadertoy 写成散落在主循环里的“临时分支逻辑”。


## 5. 一套可落地的组合方式

建议整体结构如下：

1. 平台层：`WinitPlugin`
2. 核心渲染层：`RendererCorePlugin`（设备、资源管理、RenderGraph 基础设施）
3. 功能插件层：
   - `RasterPipelinePlugin`
   - `RayTracingPipelinePlugin`
   - `ShaderToyPipelinePlugin`
   - `ImguiPlugin`
4. 策略层：`PipelineManagerPlugin`

每帧流程：

`Main Update -> Extract -> Prepare -> Queue -> RenderGraph Execute -> ImGui Overlay -> Present`

这套结构可保证：

- 插件职责清晰（谁产数据、谁做 GPU 工作、谁负责策略）
- 平台后端与渲染实现解耦（winit 可替换）
- 多管线并存且可替换（支持 capability 检查和自动回退）
- UI 叠加能力独立演进（不绑死某条主渲染管线）


## 6. 结论

- Plugin 在渲染器里应承担“能力声明与装配”角色，而非“主循环控制器”
- ImGui 应作为独立 UI 渲染特性插件接入生命周期，并做输入桥接 + overlay pass
- Winit 应作为平台后端插件，只负责窗口/事件/句柄生命周期
- 多渲染管线通过统一契约 + 独立插件 + `PipelineManager` 实现并存、切换和回退

本质上，这是把渲染器从“单体过程式控制”演进为“分层 + 分阶段 + 可装配”的架构。
