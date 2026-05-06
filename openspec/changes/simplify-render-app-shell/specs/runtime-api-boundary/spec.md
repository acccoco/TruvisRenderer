## ADDED Requirements

### Requirement: RenderApp SHALL 成为 render loop 外部契约

`RenderApp` SHALL 作为渲染线程主循环看到的 object-safe app 契约，覆盖窗口绑定初始化、单帧推进、输入事件灌入、swapchain 重建、帧节流判断和关闭。

#### Scenario: render loop 持有 RenderApp trait object

- **WHEN** winit app 创建渲染线程 app 实例
- **THEN** 它 SHALL 获得 `Box<dyn RenderApp>`
- **AND** render loop SHALL 只通过 `RenderApp` 方法驱动 app 生命周期

#### Scenario: RenderApp 是当前 FrameApp 的重命名

- **WHEN** 文档或代码引用 `RenderApp`
- **THEN** 该名称 SHALL 表示当前渲染线程 app 契约
- **AND** SHALL NOT 表示已下线的历史兼容 `RenderApp` 接口

### Requirement: RenderAppShell SHALL 实现 RenderApp

`RenderAppShell<A>` SHALL 为所有 `A: RenderAppHooks` 提供标准 `RenderApp` 实现，并作为 demo app 接入 render loop 的默认适配器。

#### Scenario: demo app 通过 shell 接入

- **WHEN** `triangle`、`rt-cornell`、`rt-sponza` 或 `shader-toy` 创建 app
- **THEN** 入口 SHALL 使用 `RenderAppShell::new(DemoApp::default())` 或等价构造
- **AND** demo app 自身 SHALL NOT 手写 `RenderApp` impl

### Requirement: RenderAppHooks SHALL 通过 typed contexts 访问 backend 能力

`RenderAppHooks` SHALL 使用 `RenderAppInitCtx`、`RenderBackendUpdateCtx`、`RenderBackendRenderCtx`、`RenderAppResizeCtx` 等受控上下文访问 runtime/backend 能力，而非直接接收完整 `RenderBackend`。

#### Scenario: hook 签名使用上下文类型

- **WHEN** 定义或实现 `RenderAppHooks` 的 init、update、render、on_resize hook
- **THEN** 每个 hook SHALL 接收对应阶段的 typed context
- **AND** hook 签名中 SHALL NOT 暴露完整 `RenderBackend`

#### Scenario: App 裁剪 Plugin Ctx

- **WHEN** app-owned plugin 需要初始化、更新、渲染或 resize
- **THEN** 具体 app SHALL 从 `RenderAppHooks` 收到的 ctx 裁剪出对应 `Plugin*Ctx`
- **AND** plugin SHALL NOT 直接依赖 `RenderBackend` 内部字段布局

### Requirement: BaseApp public API SHALL be removed

`BaseApp` SHALL NOT remain part of the public `truvis-frame-runtime` API after its responsibilities are merged into `RenderAppShell`.

#### Scenario: runtime exports no BaseApp

- **WHEN** 检查 `truvis-frame-runtime` 的 public exports
- **THEN** SHALL NOT 能通过 `truvis_frame_runtime::BaseApp` 导入帧骨架类型
- **AND** documentation SHALL describe `RenderAppShell` as the shared frame skeleton owner

## MODIFIED Requirements

### Requirement: compatibility window SHALL 具备可执行收口

兼容窗口 SHALL 以可验证条件结束，避免旧接口长期滞留。历史 `OuterApp` / `LegacyOuterAppAdapter` / `WinitApp::run` 兼容层 SHALL 保持下线；新的 `RenderApp` 名称 SHALL 表示当前 `FrameApp` 的重命名，而非恢复历史兼容接口。

#### Scenario: 兼容层下线

- **WHEN** runtime API 收敛完成
- **THEN** `OuterApp` / `LegacyOuterAppAdapter` / `WinitApp::run` SHALL 被移除或彻底下线
- **AND** 旧 `RenderApp` 兼容接口 SHALL NOT 被恢复
- **AND** 当前 render-loop trait SHALL 以 `RenderApp` 名称存在

#### Scenario: truvis-app shim 全部下线后不残留 re-export 模块

- **WHEN** 兼容窗口关闭，所有 re-export shim 被移除
- **THEN** `truvis-app` 的 `lib.rs` SHALL NOT 包含仅由 `pub use other_crate::*` 构成的纯转发模块
- **AND** `truvis-app` 的 `Cargo.toml` SHALL NOT 保留仅因 re-export 而存在的依赖项
- **AND** `truvis-app/src/render_pipeline/` SHALL 仅保留属于应用集成层的 pass 编排

## REMOVED Requirements

### Requirement: FramePlugin SHALL 通过 typed contexts 访问运行时能力

**Reason**: The app-facing orchestration contract is no longer `FramePlugin`; concrete apps implement `RenderAppHooks` and app-owned reusable units implement `Plugin`.

**Migration**: Use `RenderAppHooks` for shell callbacks and construct `PluginInitCtx` / `PluginUpdateCtx` / `PluginRenderCtx` / `PluginResizeCtx` inside the concrete app.

### Requirement: FrameRuntime SHALL 成为帧编排单入口

**Reason**: The single frame orchestration entry is now `RenderAppShell` implementing `RenderApp`.

**Migration**: Drive apps through `Box<dyn RenderApp>` and construct standard apps with `RenderAppShell`.
