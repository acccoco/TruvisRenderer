# `truvis-asset` / `gfx_resource_manager` 命名辨析（2026-04-23）

本文记录一次命名讨论：`truvis-asset` 和 `gfx_resource_manager` 都带有“资源 / 资产”的含义，
但它们面向的层级不同。如果两个名字都使用 asset/resource/manager 这类宽泛词，
后续很容易让“内容来源”和“GPU 运行时对象”混在一起。

本文只讨论命名语义，不要求立即执行重命名。

---

## 1. 核心判断

这两个概念应该拆成两层来理解：

```text
内容来源层：
  描述模型、贴图、材质、场景文件、导入状态、资产元数据。
  回答“内容从哪里来、如何被描述、如何被导入”。

GPU 运行时层：
  持有 buffer / image / image view / sampler / pipeline 等图形对象。
  回答“渲染系统如何创建、缓存、查询、复用和销毁 GPU 对象”。
```

因此，`truvis-asset` 的名字最好避免继续强调“运行时资源”；
`gfx_resource_manager` 的名字则应该明确它是图形设备侧资源的注册表、缓存或池。

---

## 2. 当前名字的问题

### 2.1 `truvis-asset`

`asset` 这个词可以成立，但它语义仍然偏宽：

- 可以表示源文件、导入结果、运行时贴图、GPU image，甚至 shader 可见资源。
- 当前 `AssetHub` 已经处在内容加载与 GPU 上传的交界处，名字越宽，越容易继续吸收边界外职责。
- 当系统后续引入 `RenderAsset`、`GpuAsset`、`AssetServer`、`AssetCache` 等概念时，`truvis-asset` 的边界会变得不够锋利。

更理想的方向是让该 crate 名字表达“内容来源 / 导入 / 内容库”，而不是表达“GPU 资源”。

### 2.2 `gfx_resource_manager`

`resource_manager` 的问题是太像兜底对象：

- `resource` 没有说明是 CPU 资源、资产资源、RenderGraph 资源，还是 GPU 资源。
- `manager` 暗示它可以负责创建、缓存、生命周期、调度、策略，职责边界偏松。
- 仓库里已经存在 RenderGraph resource、asset、scene data 等多个“资源”语境，单独叫 `resource_manager` 不够可定位。

它实际更像 GPU 对象的集中注册表 / 缓存：外部用 handle 注册、查询、复用 image view，并在合适时机清理对象。

---

## 3. 推荐命名

优先推荐：

| 当前名称 | 推荐名称 | 语义 |
|---|---|---|
| `truvis-asset` | `truvis-content` | 面向引擎输入内容、资产描述、导入状态 |
| `gfx_resource_manager` | `gfx_resource_cache` | 面向 GPU / gfx 运行时对象的持有、缓存、复用 |

对应类型可以是：

```text
truvis_content::ContentHub
truvis_render_interface::gfx_resource_cache::GfxResourceCache
```

选择这组名字的原因：

- `content` 比 `asset` 更偏创作内容和源数据，不容易被理解成 GPU 运行时对象。
- `resource_cache` 比 `resource_manager` 更克制，突出缓存、复用、生命周期记录，而不是全能管理器。
- 两者放在一起时语义互补：content 是“输入内容”，resource cache 是“运行时图形对象”。

---

## 4. 备选方案

### 4.1 `truvis-import` + `gfx_resource_pool`

适用条件：

- `truvis-asset` 的主要职责进一步收窄为 glTF / 贴图 / 材质等导入与转换。
- GPU 侧对象更强调分配、回收、重用，而不是仅仅注册和查询。

评价：

```text
truvis-import        很清楚，但如果还承担资产状态缓存，会略窄。
gfx_resource_pool    适合强调复用和回收，但不一定覆盖 image view cache / handle registry 的语义。
```

### 4.2 `truvis-scene_io` + `gfx_resource_registry`

适用条件：

- 内容层主要围绕场景文件读写、模型导入、序列化展开。
- GPU 资源侧更强调 handle/id 注册和查询，而不是缓存策略。

评价：

```text
truvis-scene_io          很具体，但会把非场景资产压进 scene 语义。
gfx_resource_registry    边界清晰，适合 handle registry；如果负责销毁和复用，则语义略窄。
```

### 4.3 `truvis-content_pipeline` + `gfx_device_resources`

适用条件：

- 后续会引入 bake、压缩、预处理、打包、热更新等完整内容管线。
- GPU 侧资源想强调其归属于 graphics device。

评价：

```text
truvis-content_pipeline    语义完整，但作为当前 crate 名可能偏重。
gfx_device_resources       明确是 device 层对象，但作为模块名不如 cache/registry 表达职责。
```

### 4.4 保留 asset，但加层级限定

如果希望保留 `asset` 这个词，可以考虑：

```text
truvis_asset_io
gfx_runtime_resources
```

评价：

- `asset_io` 比 `asset` 更明确，但如果内部有运行时状态，名字又会偏窄。
- `runtime_resources` 能区分 asset/resource 层级，但 `runtime` 不如 `gfx` 或 `device` 精确。

---

## 5. 不推荐的名字

### 5.1 `AssetManager`

不推荐原因：

- 它几乎可以装进任何资产相关逻辑。
- 很容易同时管理文件路径、CPU 缓存、GPU 资源、bindless handle、热更新状态。
- 后续拆分 `AssetServer` / `AssetCache` / `RenderAsset` 时会变成命名阻力。

### 5.2 `ResourceManager`

不推荐原因：

- 在渲染器里“resource”至少有资产资源、GPU 资源、RenderGraph 逻辑资源、descriptor 资源几层含义。
- 不带 `gfx` / `gpu` / `device` 限定时，调用点很难看出它管理的是哪一层。
- `manager` 容易鼓励继续塞入策略和调度逻辑。

---

## 6. 与现有架构文档的关系

这次命名讨论和当前几篇 brain-storm 文档是一致的：

- `2026-04-23-structure-responsibility-open-source-comparison.md` 里已经指出 `truvis-asset` 直接碰 bindless 是职责混合点。
- `2026-04-23-assets-bindless-decoupling.md` 讨论了 `AssetHub` 与 `BindlessManager` 解耦。
- `naming-renderworld-renderer-backend-app.md` 已经把 `RenderWorld` 定义为 GPU 侧状态容器，里面包含 `GfxResourceManager`。

因此，本命名建议不是单独追求好听，而是服务于同一个分层目标：

```text
content / scene / world      描述 CPU 侧内容和语义
extract / prepare            把 CPU 语义转换成 GPU 可见数据
render world / gfx cache     持有 GPU 侧状态和设备资源
renderer / frame runtime     执行和编排帧生命周期
```

---

## 7. 推荐决策快照

短期最稳妥的命名方向：

```text
truvis-asset
  -> truvis-content

gfx_resource_manager
  -> gfx_resource_cache

GfxResourceManager
  -> GfxResourceCache
```

如果暂时不做 crate/module 重命名，也建议在文档和注释中先统一语义：

```text
AssetHub / truvis-asset:
  内容来源层，负责资产加载、导入状态和内容句柄。

GfxResourceManager:
  GPU 运行时资源缓存，负责图形对象 handle、查询、复用和销毁。
```

后续真正重命名时，可以按最小改动顺序推进：

1. 先改 Rust 类型名：`GfxResourceManager` -> `GfxResourceCache`。
2. 再改模块名：`gfx_resource_manager` -> `gfx_resource_cache`。
3. 最后评估 crate 名：`truvis-asset` -> `truvis-content`。

这样可以先收紧最容易误导的运行时对象命名，再处理影响面更大的 crate rename。
