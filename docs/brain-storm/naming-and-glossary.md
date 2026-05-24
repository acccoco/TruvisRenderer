# 命名与术语表

> 状态：活跃摘要，更新于 2026-05-23。当前命名以代码和
> [`ARCHITECTURE.md`](../../ARCHITECTURE.md) 为准。

## 当前核心术语

| 术语 | 当前含义 |
|---|---|
| `RenderApp` | 渲染线程主循环可驱动的外部契约，位于 `truvis-app-frame`。 |
| `RenderAppShell` | 将具体 App hooks 适配成 `RenderApp` 的平台无关帧骨架。 |
| `RenderAppHooks` | 具体 App 实现的 init / input / update / render / resize / shutdown 回调。 |
| `Plugin` | 由具体 App 持有的可复用能力单元，覆盖标准生命周期，不负责特有 render graph API。 |
| `RenderRuntime` | GPU 运行时集成层，持有 `World`、`GpuStore`、runtime 私有 `GpuScene`、present、cmd、sync 和 manager / bridge。 |
| `World` | CPU 语义聚合层，当前持有 `SceneManager + AssetHub`。 |
| `GpuStore` | GPU-facing 状态容器，保存 manager、FIF resources、frame state、pipeline settings 等。 |
| `AssetHub` | 内容资产身份、去重、CPU 加载状态和加载事件来源。 |
| `AssetTextureManager` | render-side texture GPU upload、image/view、bindless SRV 与 fallback resolver owner。 |
| `AssetMeshManager` | render-side mesh buffer upload、BLAS build 和 mesh ready cache owner。 |
| `MaterialBridge` | asset material event 到 runtime material slot / material buffer 的桥。 |
| `InstanceBridge` | CPU runtime instance 到 stable GPU instance slot / active render data 的桥。 |
| `GpuScene` | runtime 私有 GPU scene owner，保存 scene buffers、TLAS 和 raster draw cache。 |
| `RenderSceneView` | pass 可见的只读 scene 契约，隐藏 concrete `GpuScene` owner。 |
| `RenderPresent` | runtime 持有的 surface / swapchain / present 资源 owner。 |
| `truvis-render-foundation` | 渲染基础层 crate，承载 GPU 资源状态、manager 和基础契约。 |

## 已完成命名决策

- 旧 renderer backend crate / struct 已收敛为 `truvis-render-runtime::RenderRuntime`。
- 旧 render interface crate 已收敛为 `truvis-render-foundation`。
- `GpuScene` / `RenderData` 不再属于 foundation 公开 scene 模块，而是 runtime 私有 scene 翻译层。
- pass 与上层 app 不再直接依赖 CPU world；scene 数据通过 `RenderSceneView` 暴露。

## 历史命名对照

以下名称只用于阅读归档文档：

| 历史名称 | 当前对应 |
|---|---|
| `FrameRuntime` | 现在的帧骨架主要对应 `RenderAppShell` + `RenderApp` 契约。 |
| `AppPlugin` | 现在拆为 `RenderAppHooks` 和标准 `Plugin` trait 的组合语义。 |
| `OuterApp` | 历史 App 回调接口，当前不再作为主线 API。 |
| `Renderer` backend | 当前主线命名为 `RenderRuntime`。 |
| `RenderContext` | 历史大上下文，当前已拆为 `World`、`GpuStore`、runtime 私有 owner 和 typed lifecycle Ctx。 |
| `truvis-render-interface` | 当前为 `truvis-render-foundation`。 |
| `AssetSceneHandle` / `SceneHandle` | 历史讨论中的名称，当前代码使用 model / prefab asset 语义与 explicit spawn。 |

## 命名约束

- `Asset*Handle` 表达内容资产身份，不表达 GPU 可见状态。
- `Gpu*Slot` 表达 runtime 生命周期内稳定的 GPU buffer/material/instance 位置。
- `Bindless*Handle` 表达 shader-visible descriptor index，不应进入 CPU scene 语义层。
- `View` 后续应表达渲染视角和输出意图，不等于 camera，也不拥有 scene 或 GPU manager。
- 名称应优先描述职责边界，不用 Java 风格的 “interface” 泛称承载具体实现。

## 历史来源

本文提炼自以下归档文档：

- [`archive/naming-renderworld-renderer-backend-app.md`](archive/naming-renderworld-renderer-backend-app.md)
- [`archive/2026-04-23-asset-resource-naming.md`](archive/2026-04-23-asset-resource-naming.md)
