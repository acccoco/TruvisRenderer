# Render Configuration 与 Temporal State

> 类型：设计文档。本文区分用户配置、Runtime 派生帧状态和渲染历史，避免把三者合并为一个 settings 对象。

## 三类状态

```mermaid
flowchart LR
    Config["Renderer / UI configuration"] --> Derive["Runtime derive"]
    Derive --> Frame["FrameRenderState"]
    Frame --> Targets["render/output targets"]
    View["camera / scene / settings signature"] --> History["ViewAccumState / DlssSrState"]
    History --> Temporal["temporal resources"]
```

- 配置表达用户或启动参数，希望采用什么模式。
- `FrameRenderState` 根据窗口、设备和配置推导当前尺寸与格式。
- temporal state 保存历史匹配、jitter、reset 和跨帧资源，不是配置本身。

## Runtime 配置

`DlssOptions` 由 Runtime 持有并通过 update Ctx 暴露给 Renderer。`DlssSrMode` 决定 native、DLAA 或 SR render extent；RR 是否启用由独立选项决定。

`DefaultRenderRuntimeSettings` 只描述 Runtime 初始化策略，例如默认 FPS 上限、surface/present mode 和 depth format 候选。Renderer 不应依赖设备必然选择某个格式。

## FrameRenderState

`FrameRenderState` 由 Runtime 根据真实 output extent、DLSS mode 和设备能力生成：

- `render_extent`：RT、GBuffer、motion vector、DLSS input 的内部尺寸。
- `output_extent`：swapchain、GUI、present 和 DLSS output 的尺寸。
- HDR color 与 depth format：窗口 target 和 attachment 的格式契约。

Renderer/subsystem 只读取该 state，并在 init/resize 时用它创建自身 target。它不能直接覆盖 render extent。

## Temporal state

`DlssSrState` 保存 DLSS evaluate 所需的 jitter、previous view、common constants 和 reset 标记。`ViewAccumState` 保存 main view 的历史签名和稳定帧计数。两者都由 Runtime 管理，Renderer 不直接替换内部历史。

相机、尺寸、DLSS mode、sky binding 或 scene 变化导致历史不匹配时，Runtime 请求 reset。reset 是“下一次 evaluate 不使用旧 history”的语义，不等价于立刻清除所有图像。

Realtime ReSTIR reservoir、SHARC cache、offline accumulation 属于对应 Renderer subsystem 的 temporal resources，不进入 `DlssOptions` 或 `DlssSrState`。

## Renderer 配置

`PathTracingCommonSettings` 保存 realtime/offline 共享的 sky、NEE 和 tone mapping 参数；Realtime 和 Offline 各自保存 debug channel、ReSTIR/SHARC mode 或 ray dispatch count。

同一语义只保留一个 owner：共享参数不在 realtime/offline 两个 subsystem 内各存一份，避免 UI 切换造成状态分叉。

配置变化的生效路径是：

```text
UI / startup option
-> Renderer state
-> update phase
-> Runtime derives frame state
-> optional resize / history reset
-> next prepare + render graph
```

## 非配置内容

`FrameLabel`、per-frame UBO、RenderGraph image state、resource handle 和 present image wrapper 都是运行时数据或资源视图，不表达渲染策略。

把这些对象暴露成用户配置会混淆 slot、同步和质量策略，也会使历史 reset 责任无法定位。

## 设计不变量

- 配置、派生 frame state 和 temporal history 分层保存。
- 任何尺寸变化必须经过 Runtime derive 和 resize context。
- DLSS history 不能与 Renderer 私有 accumulation history 混用。
- shared setting 只有一个语义 owner。
- mode 切换的资源释放必须满足 GPU 完成条件。

## 实现入口

- [`dlss_options.rs`](../../engine/e40-render/truvis-render-runtime/src/state/dlss_options.rs)
- [`dlss_sr.rs`](../../engine/e40-render/truvis-render-runtime/src/state/dlss_sr.rs)
- [`frame_state.rs`](../../engine/e40-render/truvis-render-runtime/src/state/frame_state.rs)
- [`view_accum.rs`](../../engine/e40-render/truvis-render-runtime/src/state/view_accum.rs)

## Reset 与生效边界

配置改变先进入 Renderer/Runtime state，再由固定 phase 收敛。UI 修改不会直接重建 Vulkan target；resize 和 feature resource 释放必须在 Runtime 的生效点处理。

历史 reset 只清除“不应被下一次 evaluate 使用”的资格。它不表示 CPU scene 被回滚，也不保证所有旧 image 立即物理清零。

如果一个设置同时影响 render extent 和 temporal history，先派生 frame state，再通知 Renderer resize，最后在同一生效路径请求 history reset，避免两者观察到不同配置。

## 变更检查

- 这个字段是策略、派生状态还是历史？
- owner 是否唯一？
- mode 变化是否需要 resize？
- 是否需要等待 GPU 后释放 feature resource？
- reset 是否覆盖 resize、camera、scene 和 lighting 变化？
- UI 是否只展示当前 Renderer 支持的配置？

## 非目标

本文不承诺所有 Renderer 都支持 DLSS、ReSTIR 或 SHARC，也不把启动环境变量视为稳定的公共配置 API。
