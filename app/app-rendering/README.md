# app-rendering

`app-rendering` 拥有与界面实现无关的 realtime/offline 渲染子系统、设置和长期 GPU 资源。

## 所有权分组

- `realtime`：`RealtimeRenderSubsystem` / `RealtimeRenderSettings`，以及 GBuffer、ReSTIR reservoir/surface key、SHARC buffer、DLSS 输入输出、working target 和 main-view target。
- `offline`：`OfflineRenderSubsystem` / `OfflineRenderSettings`，以及独立的 single-frame target、跨帧累计 image、output target、sample count 和累计签名。
- `shared`：`RenderMode`、`PathTracingCommonSettings`、`PathTracingDebugChannel`、`SkySamplingMode`、`SdrToneMappingSettings` 和 `ImageTarget`。
- `PathTracingDebugChannel` / `SkySamplingMode` 同时服务 realtime/offline；ReSTIR DI 和 SHARC 模式只属于 realtime 模块。

## 生命周期与依赖

- 每个 subsystem 实现 `app-kit::SubsystemLifecycle`，由具体 App 显式调用 `init` / `on_resize` / `shutdown` 并决定 RenderGraph pass 顺序。
- `settings()` / `settings_mut()`、`compute_cmd()` / `present_cmd()`、`contribute_compute_passes()` / `contribute_present_passes()` 和 `debug_image_options()` 保持 subsystem 自身接口。
- 窗口尺寸资源只保存 manager-owned handle；resize/shutdown 在 GPU safe point 通过 phase ctx 显式释放，不能长期保存 `Gfx`、allocator 或 runtime owner。
- offline `accum_image` 不按 FIF 轮转，不复用 realtime GBuffer、DLSS 或 ReSTIR history；只有累计签名匹配且存在 TLAS 时才推进 sample。
- 依赖 `app-kit`、`app-render-passes` 和 Engine 渲染能力，不依赖 ImGui；SDR 设置通过 shared API 暴露，避免 UI 直接依赖 pass crate。
