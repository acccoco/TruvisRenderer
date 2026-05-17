# truvis-render-backend

`truvis-render-backend` 是被 `truvis-frame-runtime::RenderAppShell` 驱动的渲染后端集成层。
它持有 `Gfx` root owner、CPU `World`、GPU `RenderWorld` 和 backend 私有的 `GpuScene`，
并通过阶段化的 typed Ctx 向上层暴露初始化、更新、渲染、resize 与 shutdown 能力。

## 职责边界

- 拥有 `Gfx` root owner，并保证所有 GPU 子资源先于 `Gfx` 销毁。
- 提供 `begin_frame`、`update_phase`、`prepare`、`render_phase`、`present`、`end_frame`、
  `handle_resize`、`shutdown_phase` 和 `destroy` 等生命周期入口。
- 产出 `RenderBackendInitCtx`、`RenderBackendUpdateCtx`、`RenderBackendRenderCtx`、
  `RenderBackendResizeCtx` 和 `RenderBackendShutdownCtx`，让上层只能在对应阶段访问窄化能力。
- 负责 CPU scene/assets 到 render-side GPU 表示的桥接，包括 texture upload、mesh upload、
  material slot、instance slot、GPU scene buffer、BLAS/TLAS 和 raster draw cache。
- 负责 surface/swapchain/present image wrapper、acquire/present semaphore 与窗口 resize 重建。
- 不负责窗口事件循环、具体 app/plugin 编排、GUI RenderGraph 适配、Assimp 文件导入或具体 pass 逻辑。

## 状态所有权

- `World` 承载 CPU 侧 `SceneManager` 与 `AssetHub`，供 update/prepare 阶段读取或修改。
- `RenderWorld` 承载 GPU 侧 frame state、global descriptors、bindless、manager-owned resources、
  FIF buffers、frame settings 和 pipeline settings。
- `GpuScene` 是 backend 私有的 scene GPU 翻译层，持有 scene/instance/geometry/light/indirect
  buffer、TLAS 和当前 FIF 的 raster draw cache；render pass 只通过 `RenderSceneView` 读取它。
- `DefaultEnvironment` 持有 sky / uv checker 等默认环境贴图，向 `GpuScene` 提供 scene root
  buffer 需要写入的 bindless handle；动态 scene 上传不再负责从路径加载默认贴图。
- `AssetTextureUploader` 消费 `AssetHub` 的 texture CPU bytes，异步上传 GPU image，并注册
  image view 与 bindless SRV；未 ready 或失败时通过 fallback texture 保证材质仍可安全读取。
- `AssetMeshUploader` 消费 `AssetHub` 的 mesh CPU 数据，在 graphics queue 上完成 vertex/index
  buffer copy 和 BLAS build；mesh 完成前不会被 `InstanceBridge` 激活。
- `MaterialBridge` 消费 `MaterialLoaded` 事件并维护 `AssetMaterialHandle -> GpuMaterialHandle` 桥接，
  底层 `MaterialManager` 负责 stable material slot、FIF material buffer、dirty region 上传、texture ready 检查和延迟 slot 回收。
- `InstanceBridge` 同步 `InstanceHandle -> GpuInstanceSlot`，在 mesh/material 都 GPU ready 前保持
  pending，并按稳定 slot 输出 active render list。
- `RenderPresent` 拥有 surface、swapchain wrapper、swapchain image/view handle 和 present 同步对象；
  app/plugin 只通过 `PresentView` / `PresentTargetView` 读取当前窗口 target 和 semaphore，不直接访问 owner 字段。

## 对外接口

- crate 公开入口保持在 `platform`、`present`、`subsystems` 和 `render_backend`。
- asset uploader、material bridge、instance bridge、GPU scene 数据结构和 prepare 辅助逻辑都是 backend 私有实现。
- 生命周期 Ctx 在 `render_backend` 内部子模块定义，并由 `render_backend` 重新导出；
  调用方仍通过 `truvis_render_backend::render_backend::*Ctx` 使用这些阶段契约。
- `RenderBackendRenderCtx` 只暴露 `RenderWorld`、`RenderSceneView`、`PresentView` 和 timeline；
  不暴露 texture/mesh uploader owner，pass 不能绕过 backend 私有 bridge 读取上传缓存。

## 生命周期

- `RenderBackend::new` 创建与窗口无关的 backend root state：`Gfx`、`World`、`RenderWorld`、
  asset uploader、bridge、`GpuScene`、FIF 资源、global descriptors、sampler 和 per-frame buffer。
- `RenderBackend::init_after_window` 在平台层提供 raw window/display handle 后创建 surface、
  swapchain 与 `RenderPresent`，并返回 init Ctx 供 app/plugin 创建长期 GPU 资源。
- `begin_frame` 是每帧资源回收入口：timer tick、等待当前 FIF slot、重置 frame command pool、
  清理延迟释放队列、推进 bindless/material/instance frame token，并在 `RenderBackend`
  内部分发 AssetHub 事件。
- `update_phase` 同步 frame settings、acquire 当前 swapchain image，并返回 CPU update Ctx。
- `prepare(camera)` 是 CPU 语义数据到 GPU 可见数据的边界：它读取 app 提供的 camera，
  在 `RenderBackend` 内部同步 material/instance/mesh/texture 状态、上传 GPU scene
  和 per-frame data，再刷新 per-frame descriptor。
- `render_phase` 返回只读 render Ctx；pass 只能读取 `RenderWorld`、`RenderSceneView`、
  present target 和 timeline，不再修改 CPU scene 或接触 uploader owner。
- `present` 只提交当前 swapchain image 到 present queue；渲染命令提交由上层 render graph 完成。
- `end_frame` 推进 frame counter，切换下一帧的 FIF label。
- `wait_idle` 在 app/plugin shutdown 前调用，确保上层资源释放时不再被 GPU command 引用。
- `destroy` 等待 GPU idle，依次释放 present、FIF、scene/assets、GPU scene、mesh uploader、
  command allocator、resource manager、sync、sampler、descriptor 等资源，最后销毁 `Gfx`。

## Prepare 数据流

- `RenderBackend::dispatch_loaded_asset_events` 将 `AssetHub::update()` 产出的 texture 事件交给 `AssetTextureUploader`，mesh 事件交给
  `AssetMeshUploader`，material 事件交给 `MaterialBridge`；scene loaded/failed 只记录日志，scene 实例化入口仍在 asset/scene 层。
- `RenderBackend::prepare` 是 update 与 render 之间的固定桥接阶段，按 bindless、material、instance、
  GPU scene、per-frame data 的顺序准备渲染可见数据。
- `MaterialBridge` 在 begin-frame 阶段消费 `MaterialLoaded` 事件并同步到 `MaterialManager`，
  prepare 阶段再通过 `TextureResolver` 把 texture fallback/ready 状态按 dirty slot 局部写入 material buffer。
- `InstanceBridge` 读取 `SceneManager`，并通过 `MaterialSlotResolver` 与 `MeshRenderResolver`
  做 ready gate，只有完整可渲染的实例才进入 `RenderData`。
- `GpuScene` 消费 `RenderData`，按当前 FIF 上传 geometry、instance、light、indirect 和 scene
  root buffer，必要时重建 TLAS，并刷新 raster draw cache。

## 同步与稳定性约束

- backend 全局 FIF timeline 确保 frame command pool 与延迟释放资源不会覆盖 GPU 仍在读取的数据。
- texture uploader 使用 transfer queue timeline semaphore 异步检测 copy 完成，不阻塞帧循环。
- mesh uploader 使用 graphics queue timeline semaphore，因为 BLAS build 不能假设 transfer queue 支持。
- mesh copy 到 BLAS build 前必须覆盖 `TRANSFER_WRITE -> ACCELERATION_STRUCTURE_BUILD_KHR`，
  并包含 device address 输入对应的 `SHADER_READ` 访问。
- material slot 与 instance slot 都延迟到跨过 FIF 窗口后才回收，避免在飞命令中的旧索引指向新对象。
- mesh ready revision 与 instance revision 合成 scene revision，`GpuScene` 只在当前 FIF 的 TLAS 过期时重建。
- swapchain resize 采用 latest-size 标记；窗口事件只记录最新尺寸，实际重建延迟到 render loop 的安全点。

## Tracy 初始化埋点

- `RenderBackend::new` 使用一级 span 标记主要初始化阶段，例如 `Gfx`、manager、asset uploader、
  material bridge、GPU scene、FIF buffers、global descriptors、sampler、per-frame buffer 和 command buffer。
- 启动耗时较明显的下层构造函数继续使用二级 span 细分，例如 `AssetTextureUploader::new`、
  `GpuScene::new`、`FifBuffers::new`、`GlobalDescriptorSets::new`、`CmdAllocator::new`
  和 `RenderSamplerManager::new`。
- `SceneManager::new` 不在 `truvis-scene` 内部添加 Tracy 依赖；它只通过
  `RenderBackend::new/scene_manager` 这个一级 span 表示。
