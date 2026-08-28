# RenderGraph 与帧内数据流

> 状态：当前实现事实总结。本文记录 RenderGraph 的资源声明、线性执行、同步、典型 graph 组织和 owner 边界。

## 机制定位

`RenderRuntime::prepare` 是 CPU scene 到 GPU scene 的固定同步边界；完整身份转换、upload 和 ready/fallback 规则见
[`scene-data-lifecycle.md`](scene-data-lifecycle.md)。prepare 完成后，Renderer 在 render hook 中读取
`RenderSceneView` 和 runtime render state，构造本帧 RenderGraph；render pass 不再访问 `World`、`SceneStore`
或 render-side manager owner。

RenderGraph 是按 Renderer 指定顺序执行的命令录制与同步辅助，不是自动调度器：

- `RenderGraphBuilder` 按 pass 添加顺序记录执行序列，不做拓扑排序或 pass 重排。
- Renderer 决定完整业务顺序；具体渲染子系统只贡献自己的 pass 或 sub-flow。
- 当前 graph 只跟踪 imported image 的访问与状态，不创建或拥有 image。
- graph compile 通过线性扫描生成 image barrier、layout transition、epilogue barrier 和 semaphore submit 信息。
- execution 通过 `GfxResourceAccess` 解析 image/image-view handle，不依赖 concrete `GfxResourceManager`。

## 资源职责

同一张固定用途的渲染 image 同时涉及三个不同职责，不能互相替代：

1. **Subsystem owner**：创建并持有 render target、GBuffer、DLSS 输入输出、累计图、selection mask 或 ImGui font，
   并在 init/resize/shutdown 的 GPU-safe 时机释放。
2. **Pass-local descriptor**：描述本次 draw/dispatch 使用哪个 image view；descriptor 不拥有 image，也不负责同步。
3. **RenderGraph state declaration**：声明 sampled/storage/color attachment 等访问，负责 pass 间 barrier 和 layout transition。

descriptor 中的 image layout 必须与执行该 pass 时 graph 推导出的状态一致。storage image 固定使用 `GENERAL`，
sampled image 固定使用 `SHADER_READ_ONLY_OPTIMAL`；固定用途的渲染 image 通过 local push descriptor 引用，
不注册到全局 bindless table。

全局 bindless set 只保存 Material/Scene 数据动态索引的 asset texture 与 sky sampled-image SRV。窗口 resize
只重建具体 subsystem owner 的 target 和 pass-local 引用，不产生全局 bindless slot 注册/回收压力。

## Pass 顺序与状态推导

pass 使用 `read_image`、`write_image` 或 `read_write_image` 声明资源访问。声明参与 barrier 推导、状态校验和
调试输出，但不参与调度。image 初始状态来自 import；后续线性扫描处理：

- write-after-read、read-after-write 与 write-after-write；
- layout transition；
- 连续只读访问；
- graph 末尾需要恢复的 exported/imported image 状态；
- imported image 的 wait semaphore 与 exported image 的 signal semaphore。

present image 通过 `PresentView::import_current_target` 接入 graph。acquire 和 render-complete semaphore 由
present adapter 固定提供，Renderer/子系统不直接访问 `SwapchainPresenter` owner 或同步对象。

同步 raycast 不进入 RenderGraph。它在 prepare 后、render graph 组图前通过 runtime-owned pipeline、command pool 和
fence 提交，阻塞读取当前 GPU scene snapshot，再把 GPU instance slot/submesh 转回 CPU scene handle。

## 典型 Graph 组织

Triangle 与 ShaderToy 使用单个 present graph。主体 RT Renderer 通常分成 compute graph 与 present graph：

```text
prepare 后的 RenderSceneView
  -> rendering subsystem compute/RT passes
  -> main view image
  -> present resolve
  -> Renderer-owned overlays/effects
  -> ImGui pass
  -> present
```

Truvis 根据 `RenderMode` 选择一个 renderer-owned 渲染子系统：

- Realtime：`RealtimeRenderSubsystem` 贡献 realtime ray tracing、可选 DLSS SR/RR 与后处理；RT path 内部可启用 ReSTIR DI 和 SHARC。
- Offline：`OfflineRenderSubsystem` 贡献独立 RT dispatch、累计和 output target，不复用 realtime temporal state。

realtime light candidate、MIS、ReSTIR 和 SHARC 的算法与 shader 数据契约由
[`realtime-rt-raytracing-flow.md`](realtime-rt-raytracing-flow.md) 统一说明，本文不复制算法细节。

主体 Renderer 的 present 顺序由 `TruvisRenderer` 显式决定：先 resolve main view，再组合 selection outline、
coordinate gizmo 和 ImGui。具体 Renderer owner 与编排入口见
[`renderer/truvis/README.md`](../../renderer/truvis/README.md)。

离线累计 image 是 offline subsystem-owned 跨帧历史，不按 FIF 轮转；per-FIF output target 仍按当前 `FrameLabel` 使用。
没有 TLAS 时，OfflineRenderSubsystem 重置累计并把相关 target 清为确定黑色，避免展示未定义或过期结果。

## 调试与当前能力边界

`CompiledGraph::print_execution_plan()` 输出 pass 顺序、image 访问、pass 前 barrier、epilogue barrier 和
semaphore 数量，用于核对声明与实际录制顺序。

当前 RenderGraph 不提供：

- transient image/buffer 分配或资源 aliasing；
- buffer/acceleration-structure 状态跟踪；
- pass culling、自动拓扑调度或多队列 scheduler；
- runtime prepare、asset upload 或同步 raycast 的统一编排。

这些能力如需演进，方案与非目标记录在
[`docs/brain-storm/render-graph-evolution.md`](../brain-storm/render-graph-evolution.md)。

## 与帧生命周期的关系

- update：Renderer/子系统可以修改 CPU scene 与 renderer-owned 设置。
- prepare：runtime 把 CPU scene、asset 和 view 快照同步为 GPU 可见状态。
- after_prepare：只处理依赖当前 prepared scene 的同步查询。
- render：Renderer 构造 RenderGraph，pass 只读取 prepared GPU scene 和 renderer/runtime render state。
- submit/present：compiled graph 生成提交信息，present owner 完成 swapchain present。
