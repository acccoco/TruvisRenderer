# truvis-render-foundation

跨渲染 crate 的最小契约层。它只提供上层和 RenderGraph 都需要共享的轻量类型，不持有 GPU owner，不负责 descriptor、bindless、command pool 或 per-frame GPU buffer 生命周期。

## 关键组件

- `FrameLabel`
- `GfxImageHandle` / `GfxImageViewHandle` / `GfxBufferHandle`
- `RenderView`
- `RenderSceneView`
- `GfxResourceAccess`

## 契约边界

- `FrameLabel` 只表达 A/B/C 三个 FIF slot 及其固定数量和轮转映射；帧序号、时间快照和延迟回收使用的当前 frame id 由 `truvis-render-runtime::FrameTiming` 持有。
- `RenderView` 是 app 相机状态到 runtime prepare 阶段的纯数据快照；runtime 不依赖 app camera 的具体存储和输入控制方式。
- `RenderSceneView` 是 render pass 访问 runtime 私有 GPU scene 的窄只读契约；concrete `RenderWorld`、`RenderData`、稳定 instance slot 与 raster draw cache 属于 `truvis-render-runtime`。
- `GfxResourceAccess` 是 RenderGraph 解析 imported image/image view 句柄的只读查询契约；具体 `GfxResourceManager` 实现在 `truvis-render-runtime`。

## 非职责

- 不拥有 `GfxResourceManager`、`ShaderBindingSystem`、`GlobalDescriptorSets`、`BindlessManager`、`CmdAllocator` 或 `PerFrameGpuData`。
- 不创建、注册、释放 Vulkan/VMA/WSI 资源。
- 不依赖 App、具体子系统、scene loading、窗口平台或 runtime render state 语义。

## 依赖原则

foundation 位于 `truvis-gfx` 之上、`truvis-render-graph` 和 `truvis-render-runtime` 之下。它可以引用 RHI 资源类型以定义只读契约，但不能反向依赖 runtime 或 app 层实现。
