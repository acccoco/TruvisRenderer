# truvis-render-runtime

`truvis-render-runtime` 是被 `truvis-app-frame::RenderAppShell` 驱动的渲染运行时集成层。
它持有 `Gfx` root owner、CPU `World`、GPU `GpuStore` 和 runtime 私有的 `GpuScene`，
并通过阶段化的 typed Ctx 向上层暴露初始化、更新、渲染、resize 与 shutdown 能力。

## 职责边界

- 拥有 `Gfx` root owner，并保证所有 GPU 子资源先于 `Gfx` 销毁。
- 提供 `begin_frame`、`update_phase`、`prepare`、`render_phase`、`present`、`end_frame`、
  `handle_resize`、`shutdown_phase` 和 `destroy` 等生命周期入口。
- 产出 `RenderRuntimeInitCtx`、`RenderRuntimeUpdateCtx`、`RenderRuntimeRenderCtx`、
  `RenderRuntimeRayCastCtx`、`RenderRuntimeResizeCtx` 和 `RenderRuntimeShutdownCtx`，让上层只能在对应阶段访问窄化能力。
- 负责 CPU scene/assets 到 render-side GPU 表示的桥接，包括 texture upload、mesh upload、
  material slot、instance slot、GPU scene buffer、BLAS/TLAS 和 raster draw cache。
- 在 `prepare` 完成后提供 runtime-owned 同步 raycast 服务，把 GPU hit 的 instance slot
  与 submesh index 转回 CPU `InstanceHandle` / asset handle。
- 负责 surface/swapchain/present image wrapper、acquire/present semaphore 与窗口 resize 重建。
- 不负责窗口事件循环、具体 app/plugin 编排、GUI RenderGraph 适配、Assimp 文件导入或具体 pass 逻辑。

## 状态所有权

- `World` 承载 CPU 侧 `SceneManager` 与 `AssetHub`，供 update/prepare 阶段读取或修改。
- `GpuStore` 承载 GPU 侧 frame state、global descriptors、bindless、manager-owned resources、
  FIF buffers、frame settings 和 pipeline settings。
- `GpuScene` 是 runtime 私有的 scene GPU 翻译层，持有 scene/instance/geometry/light/indirect
  buffer、TLAS 和当前 FIF 的 raster draw cache；render pass 只通过 `RenderSceneView` 读取它。
- `DefaultEnvironment` 持有 sky / uv checker 等默认环境贴图，向 `GpuScene` 提供 scene root
  buffer 需要写入的 bindless handle；动态 scene 上传不再负责从路径加载默认贴图。
- `AssetTextureManager` 消费 `AssetHub` 的 texture CPU bytes，异步上传 GPU image，并注册
  image view 与 bindless SRV；未 ready 或失败时通过 fallback texture 保证材质仍可安全读取。
- `AssetMeshManager` 消费 `AssetHub` 的 mesh CPU 数据，在 graphics queue 上完成 vertex/index
  buffer copy 和 BLAS build；mesh 完成前不会被 `InstanceBridge` 激活。
- `MaterialBridge` 消费 `MaterialLoaded` 事件并维护 `AssetMaterialHandle -> GpuMaterialHandle` 桥接，
  底层 `MaterialManager` 负责 stable material slot、FIF material buffer、dirty region 上传、texture ready 检查和延迟 slot 回收。
- `InstanceBridge` 同步 `InstanceHandle -> GpuInstanceSlot`，在 mesh/material 都 GPU ready 前保持
  pending，并按稳定 slot 输出 active render list，同时为同步 raycast 生成当前 prepare 快照的 slot 反查表。
- `RayCastService` 持有专用 ray tracing pipeline/SBT、可增长 ray/result/readback buffer、
  command pool 和 fence；它由 runtime 拥有，不进入 RenderGraph。
- `RenderPresent` 拥有 surface、swapchain wrapper、swapchain image/view handle 和 present 同步对象；
  app/plugin 只通过 `PresentView` / `PresentTargetView` 读取当前窗口 target 和 semaphore，不直接访问 owner 字段。

## 对外接口

- crate 公开入口保持在 `platform`、`present`、`render_runtime_ctx` 和 `render_runtime`；
  `platform` 只保留默认相机等上层需要显式传入 runtime 的轻量类型。
- asset manager、material bridge、instance bridge、GPU scene 数据结构和 prepare 辅助逻辑都是 runtime 私有实现。
- 生命周期 Ctx 在 `render_runtime_ctx` 模块定义，并由 `render_runtime` 重新导出；
  调用方仍通过 `truvis_render_runtime::render_runtime::*Ctx` 使用这些阶段契约。
- `RenderRuntimeRenderCtx` 只暴露 `GpuStore`、`RenderSceneView`、`PresentView` 和 timeline；
  不暴露 texture/mesh manager owner，pass 不能绕过 runtime 私有 bridge 读取上传缓存。
- `RenderRuntimeRayCastCtx` 只暴露同步批量 raycast 调用；App 应在 `after_prepare`
  阶段使用它，update/input 阶段不提供该接口。

## 生命周期

- `RenderRuntime::new` 创建与窗口无关的 runtime root state：`Gfx`、`World`、`GpuStore`、
  asset manager、bridge、`GpuScene`、FIF 资源、global descriptors、sampler 和 per-frame buffer。
- `RenderRuntime::init_after_window` 在平台层提供 raw window/display handle 后创建 surface、
  swapchain 与 `RenderPresent`，并返回 init Ctx 供 app/plugin 创建长期 GPU 资源。
- `begin_frame` 是每帧资源回收入口：推进 runtime 私有帧计时器、等待当前 FIF slot、重置 frame command pool、
  清理延迟释放队列、推进 bindless/material/instance frame token，并在 `RenderRuntime`
  内部分发 AssetHub 事件。
- `update_phase` 同步 frame settings、acquire 当前 swapchain image，并返回 CPU update Ctx。
- `prepare(camera)` 是 CPU 语义数据到 GPU 可见数据的边界：它读取 app 提供的 camera，
  在 `RenderRuntime` 内部同步 material/instance/mesh/texture 状态、上传 GPU scene
  和 per-frame data，再刷新 per-frame descriptor。
- `ray_cast_phase` 发生在 `prepare` 之后、`render_phase` 之前。同步 raycast 提交到
  graphics queue，并用 fence 阻塞等待 readback；队列顺序保证它能看到本帧 prepare
  提交的 GPU scene/TLAS。
- `render_phase` 返回只读 render Ctx；pass 只能读取 `GpuStore`、`RenderSceneView`、
  present target 和 timeline，不再修改 CPU scene 或接触 manager owner。
- `present` 只提交当前 swapchain image 到 present queue；渲染命令提交由上层 render graph 完成。
- `end_frame` 推进 frame counter，切换下一帧的 FIF label。
- `wait_idle` 在 app/plugin shutdown 前调用，确保上层资源释放时不再被 GPU command 引用。
- `destroy` 等待 GPU idle，依次释放 present、FIF、scene/assets、GPU scene、mesh manager、
  command allocator、resource manager、sync、sampler、descriptor 等资源，最后销毁 `Gfx`。

## Prepare 数据流

- `RenderRuntime::dispatch_loaded_asset_events` 将 `AssetHub::update()` 产出的 texture 事件交给 `AssetTextureManager`，mesh 事件交给
  `AssetMeshManager`，material 事件交给 `MaterialBridge`；model ready/failed 状态由 App 通过 `AssetHub` 查询，实例化入口在 `SceneManager`。
- `RenderRuntime::prepare` 是 update 与 render 之间的固定桥接阶段，按 bindless、material、instance、
  GPU scene、per-frame data 的顺序准备渲染可见数据。
- `MaterialBridge` 在 begin-frame 阶段消费 `MaterialLoaded` 事件并同步到 `MaterialManager`，
  prepare 阶段再通过 `TextureResolver` 把 texture fallback/ready 状态按 dirty slot 局部写入 material buffer。
- `InstanceBridge` 读取 `SceneManager`，并通过 `MaterialSlotResolver` 与 `MeshRenderResolver`
  做 ready gate，只有完整可渲染的实例才进入 `RenderData`。
- `InstanceBridge` 在同一次 prepare 输出中同步生成 `GpuInstanceSlot -> CPU record`
  反查快照。raycast readback 只信任这个快照，避免查询阶段重新遍历 CPU scene。
- `GpuScene` 消费 `RenderData`，按当前 FIF 上传 geometry、instance、light、indirect 和 scene
  root buffer，必要时重建 TLAS，并刷新 raster draw cache。

## 同步与稳定性约束

- runtime 全局 FIF timeline 确保 frame command pool 与延迟释放资源不会覆盖 GPU 仍在读取的数据。
- texture manager 使用 transfer queue timeline semaphore 异步检测 copy 完成，不阻塞帧循环。
- mesh manager 使用 graphics queue timeline semaphore，因为 BLAS build 不能假设 transfer queue 支持。
- mesh copy 到 BLAS build 前必须覆盖 `TRANSFER_WRITE -> ACCELERATION_STRUCTURE_BUILD_KHR`，
  并包含 device address 输入对应的 `SHADER_READ` 访问。
- material slot 与 instance slot 都延迟到跨过 FIF 窗口后才回收，避免在飞命令中的旧索引指向新对象。
- mesh ready revision 与 instance revision 合成 scene revision，`GpuScene` 只在当前 FIF 的 TLAS 过期时重建。
- 同步 raycast 是阻塞接口，适合拾取、编辑器选择等即时交互，不适合作为每帧大规模查询队列。
  结果语义是视觉拾取：closest hit shader 返回可见表面，any-hit 会按材质 opacity / diffuse alpha 忽略透明命中。
- swapchain resize 采用 latest-size 标记；窗口事件只记录最新尺寸，实际重建延迟到 render loop 的安全点。

## Tracy 初始化埋点

- `RenderRuntime::new` 使用一级 span 标记主要初始化阶段，例如 `Gfx`、manager、asset manager、
  material bridge、GPU scene、FIF buffers、global descriptors、sampler、per-frame buffer 和 command buffer。
- 启动耗时较明显的下层构造函数继续使用二级 span 细分，例如 `AssetTextureManager::new`、
  `GpuScene::new`、`FifBuffers::new`、`GlobalDescriptorSets::new`、`CmdAllocator::new`
  和 `RenderSamplerManager::new`。
- `SceneManager::new` 不在 `truvis-world` 内部添加 Tracy 依赖；它只通过
  `RenderRuntime::new/scene_manager` 这个一级 span 表示。
