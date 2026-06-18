# RenderGraph 与数据流

> 状态：当前实现事实总结。本文记录 CPU 语义数据到 GPU scene 的同步路径，以及 RenderGraph 的 pass 编排规则。

## CPU 资产到 GPU Scene

CPU 语义数据从 `World` 进入 RenderRuntime。资产加载先由 `AssetHub` 产出 upload-ready CPU bytes，再由 `AssetTextureManager`
在渲染线程上传到 GPU 并注册 bindless。

material 参数由 `AssetHub` 通过 `MaterialLoaded` 事件交给 render-side `MaterialBridge`，再由 runtime 显式传入
`MaterialManager` 完成注册或更新。mesh CPU 数据由 `AssetMeshManager` 在 graphics queue 上上传 vertex/index buffer 并构建 BLAS。

Assimp model 读取由 `AssetHub::load_model` 在后台完成，只保存 `ModelData` / prefab CPU 数据；App 根据 ready 的 model data
显式调用 `SceneManager::spawn_model`。`SceneManager` 保存运行时语义，`InstanceBridge` 负责稳定 GPU instance slot、ready
gate 和 active render list。

mesh/material ready 查询通过 `truvis-render-runtime` 私有 scene bridge trait 连接到 `AssetMeshManager` 与
`MaterialBridge` / `MaterialManager` 组合 resolver，这些 resolver 不属于 `truvis-world`。

默认 sky 通过 `SkyBridge` 请求 `AssetHub::load_texture` 异步加载，并在真实贴图 GPU ready 前使用常驻纯色 fallback。
`SkyBridge` 同时在默认 sky 的 CPU texture bytes 到达时构建 HDRI importance alias table；真实 sky image 仍由
`AssetTextureManager` 上传，scene root buffer 只消费当前可用的 sky SRV、sampler 与 distribution 快照。

`GpuScene`、`EmissiveLightTable` 与 `RenderData` 是 runtime 私有 scene 翻译层；render pass 只通过
`RenderSceneView` 访问 scene buffer、TLAS handle 和光栅化 draw。

`RenderRuntime::prepare` 负责把这些 bridge 按固定顺序串成 update 与 render 之间的 prepare 阶段。`after_prepare` 阶段只用于
App 对刚同步完成的 GPU scene 发起同步查询，例如批量 raycast；普通渲染工作仍在 `render` hook 中进入 RenderGraph。

```mermaid
flowchart LR
    AssetHub["AssetHub<br/>path/key -> CPU asset data"] --> TextureManager["AssetTextureManager<br/>texture GPU upload + bindless"]
    AssetHub --> SkyBridge["SkyBridge<br/>default sky handle + fallback"]
    AssetHub --> MeshManager["AssetMeshManager<br/>mesh buffer upload + BLAS"]
    AssetHub --> MaterialBridge["MaterialBridge<br/>asset material -> GPU material handle"]
    MaterialBridge --> MaterialManager["MaterialManager(RenderRuntime)<br/>stable material slot + material buffer"]
    AssetHub --> ModelData["ModelData<br/>prefab asset"]
    ModelData --> SceneSpawner["SceneManager::spawn_model"]
    SceneSpawner --> Scene["SceneManager<br/>runtime instance / light"]
    Scene --> InstanceBridge["InstanceBridge<br/>stable instance slot + ready gate"]
    TextureManager --> MaterialManager
    TextureManager --> SkyBridge
    MeshManager --> InstanceBridge
    Scene --> Prepare["RenderRuntime::prepare(render_view)"]
    MaterialBridge --> InstanceBridge
    MaterialManager --> InstanceBridge
    MaterialManager --> Prepare
    SkyBridge --> Prepare
    InstanceBridge --> Prepare
    Prepare --> EmissiveTable["EmissiveLightTable<br/>emissive triangle records + alias"]
    MaterialManager --> EmissiveTable
    EmissiveTable --> GpuResources
    Prepare --> GpuResources["GpuScene(RenderRuntime) / BindlessManager / GlobalDescriptorSets"]
    GpuResources --> RayCast["App after_prepare<br/>sync raycast query"]
    RayCast --> BuildGraph["App render hook builds RenderGraph"]
    BuildGraph --> RecordCommands["pass command recording"]
    RecordCommands --> Submit["queue submit"]
    Submit --> Present["swapchain present"]
```

## RenderGraph 规则

- App 在 `RenderAppHooks::render` 中创建 RenderGraph。
- 同步 raycast 不接入 RenderGraph；它在 `after_prepare` 通过独立 command pool/fence 提交，阻塞读回后把 GPU instance
  slot/submesh 转回 CPU `InstanceHandle` 与 asset handle。
- 渲染管线 Plugin 只贡献自己的 pass，不决定整个 App 的完整执行顺序。
- App 显式决定 GUI pass 与渲染管线 pass 的添加顺序，RenderGraph 按该顺序录制，不做自动重排。
- pass 必须声明 image 读写状态，让 RenderGraph 在线性序列中推导同步与 layout transition。

## 典型 Graph 组织

Triangle / ShaderToy 使用单个 present graph。

RT demo 使用 compute graph 与 present graph：App 先让 `RtPipeline` 贡献 compute passes，再在 present graph 中先
resolve，最后调用 `GuiPlugin::contribute_passes` 叠加 GUI。

## RT 直接光采样契约

当前 realtime RT 主路径的直接光通过统一 Light Candidate System 接入 HDRI、自发光三角形和 analytic light。
raygen shader 每个普通 surface 只生成一个 light candidate；candidate 使用 direction、radiance、distance、
shadow ray 和 solid-angle PDF 描述光源侧样本；visibility 复用现有 inline `RayQuery` shadow path；shade 使用
`BRDF * cos / light_pdf * MIS` 的统一公式。
完整 raygen path loop、miss / emissive hit、多 bounce、Russian roulette 和 MIS 决策顺序见
[`docs/summaries/realtime-rt-raytracing-flow.md`](realtime-rt-raytracing-flow.md)。

环境光 sample 与 PDF 统一通过 `EnvMap` 查询。默认 sky 真实贴图 ready 后，`EnvMap` 使用
`SkyBridge` 生成的 `luminance(texel) * solid_angle(texel)` alias table 做 importance sampling，并返回
solid-angle PDF；fallback sky 使用 1x1 均匀分布，无效分布或 `RtPipelineSettings.sky_sampling_mode = Uniform`
时回退 uniform sphere。HDRI class 内部采样与 BRDF sky miss 读取同一 `EnvMap::pdf`，统一入口再把
light-class 选择概率乘入对外 PDF。
HDRI 采样的概念解释、alias table 原理和项目内数据路径见
[`docs/summaries/hdri-sampling.md`](hdri-sampling.md)。

自发光三角形由 `EmissiveLightTable` 在 prepare 阶段构建。`AssetMeshManager` 保留 mesh-local triangle metadata，
`MaterialManager` 提供按 material slot 查询参数的窄接口；`EmissiveLightTable` 在
`InstanceBridge::prepare_render_data` 之后、`GpuScene::upload_render_data` 之前读取 active instance/submesh，
上传 `emissive_triangle_lights`、`emissive_light_alias_table` 和 `instance_emissive_triangle_base_map`。

`instance_emissive_triangle_base_map` 与 `instance_geometry_map` / `instance_material_map` 使用同一 instance-local
submesh 顺序，索引为 `instance.geometry_indirect_idx + geometry_id`；非 emissive submesh 写 `UINT_MAX`。
emissive submesh 为所有 primitive 保留连续 `EmissiveTriangleLight` record，因此 BRDF hit emissive 时可直接用
`emissive_triangle_lights[base + primitive_id]` 反查 light-side PDF，不需要额外 lookup entry 或二分查找。
`emissive_light_alias_table` 只服务 NEE 抽样，内部 entry 保存 primary record index、alias record index 与
alias probability；hit PDF 查询不经过 alias table。自发光 NEE 的面积采样 PDF 会转换到 solid-angle 度量，
统一入口再把 light-class 选择概率乘入对外 PDF 后与 BRDF PDF 做 MIS；camera ray 或上一段 delta path
直接命中 emissive 仍保持当前直视/镜面语义。
lookup 的构建步骤、buffer 内部结构和查询伪代码见 [`docs/summaries/emissive-light-sampling.md`](emissive-light-sampling.md)。

`RtPipelineSettings.emissive_nee_enabled` 默认开启；关闭或 table 为空时统一入口不会把 emissive class 纳入候选来源。
`NeeEmissive` debug channel 只显示统一 NEE 中抽到 emissive triangle class 的贡献，HDRI class 仍由既有
`NeeHdri` 通道观察。emissive table 的 rebuild
revision 由 mesh ready revision、instance revision 和 material revision 组合得到；mesh ready、instance
transform / active set、material emissive / base color 参数变化会刷新 table。

analytic point / spot / area light 由 `SceneManager` 保存 CPU 语义记录，`InstanceBridge::prepare_render_data`
把三类 light 快照写入 `RenderData`，`GpuScene::upload_render_data` 分别上传 point / spot / area structured buffer
并在 scene root 中写入 device address 与 count。`RtPipelineSettings.analytic_nee_enabled` 开启且 light 数量非 0
时，统一入口才会把 analytic class 纳入候选来源；`NeeAnalytic` debug channel 只显示统一 NEE 中抽到 analytic
class 的贡献。更细的
sphere emitter、spot cone、area 单面 PDF 和 MIS 边界见 [`analytic-light-sampling.md`](analytic-light-sampling.md)。

DLSS SR/RR 仍只读取 RT 输出的 HDR、GBuffer、depth、motion vectors 和固定 manual exposure，不参与 light
candidate、reservoir 或 radiance cache 状态。SR 会显式 tag 1x1 `dlss-sr-exposure`，避免缺少
`kBufferTypeExposure` 时 Streamline 退回 AutoExposure；SDR `Exposure EV` 仍只作用于 DLSS 之后的 tone mapping。

## 与生命周期的关系

- update 阶段可以修改 CPU 语义状态。
- prepare 是 CPU scene / asset / material / instance 到 GPU 可见状态的同步边界。
- after_prepare 只处理刚准备好的 GPU scene 的同步查询。
- render 阶段只读取 prepare 后的 GPU scene 快照，并通过 RenderGraph 录制 pass。
