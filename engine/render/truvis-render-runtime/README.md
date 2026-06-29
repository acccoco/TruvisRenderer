# truvis-render-runtime

`truvis-render-runtime` 是被 `truvis-app-frame::RenderAppShell` 驱动的渲染运行时集成层。
它持有 `Gfx` root owner、CPU `World`、GPU resource/binding/timing owners 和 runtime 私有的 `RenderWorld`，
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
  与 submesh index 转回 CPU `InstanceHandle` / `SceneMeshHandle` / `SceneMaterialHandle`。
- 负责 surface/swapchain/present image wrapper、acquire/present semaphore 与窗口 resize 重建。
- 不负责窗口事件循环、具体 app/plugin 编排、GUI RenderGraph 适配、Assimp 文件导入或具体 pass 逻辑。

## 状态所有权

- `World` 承载 CPU 侧 `SceneStore` 与 `AssetHub`，供 update/prepare 阶段读取或修改；App-facing
  model import、procedural mesh/material、runtime instance 和 analytic light 注册通过 `World` facade 进入，
  render runtime 只通过 `World::sync_for_render` 产出的 `WorldRenderSync` typed payload、`SceneChanges` 和
  `World::scene_view()` 只读 scene snapshot 访问这些 CPU owner。
- `GfxResourceManager` 承载 manager-owned GPU image/buffer/view 生命周期。
- `ShaderBindingSystem` 承载 global descriptors、bindless 和 sampler manager，并向 render 阶段提供只读 shader binding view。
- `FrameTiming` 是 runtime-owned 当前帧时间快照，承载 frame counter、delta time 和 total time；`PerFrameGpuData` 承载 per-FIF `PerFrameData` UBO。
- `FrameRenderState`、`DlssOptions`、`ViewAccumState` 和 `DlssSrState` 定义在本 crate，
  并由 `RenderRuntime` 持有；`DlssOptions` 同时提供 SR/RR active feature 决策。
- runtime 内部拥有默认 surface format、present mode 与 depth format 候选顺序；这些默认策略不放入
  foundation 公共配置契约。
- `RenderWorld` 是 runtime 私有的 scene GPU 翻译层，内部持有 `RenderTextureManager`、`RenderMeshManager`、
  `RenderMaterialManager`、`RenderInstanceManager`、`RenderSkyManager`、`RenderEmissiveLightTable`、
  scene/instance/geometry/light/indirect buffer、raster draw cache 和 `RenderTlasManager`；render pass
  只通过 `RenderSceneView` 读取它。
- 默认 sky 通过 `World` facade 注册为普通 `SceneTextureHandle`，再写入 `SceneStore::SceneSkyState`；
  `RenderSkyManager` 从 `World::scene_view()` 读取 sky state，持有常驻纯色 fallback sky，并在当前 sky CPU texture
  bytes 到达时构建 HDRI importance distribution；`RenderWorld` 只消费 sky 环境绑定快照。
- `RenderTextureManager` 消费 `WorldRenderSync.asset_uploads.pending_texture_uploads` 的 texture CPU bytes，异步上传 GPU image，并注册
  image view 与 bindless SRV；未 ready 或失败时通过 fallback texture 保证材质仍可安全读取。
  默认 sky 的真实 texture 也复用该上传路径，但 sky fallback 由 `RenderSkyManager` 独立维护。`SceneChanges.removed_textures`
  会先移除已 ready cache；已提交但未完成的 stale upload 在 timeline 到达后只销毁，不会重新 publish 到 resolver。
- `RenderMeshManager` 消费 `WorldRenderSync.asset_uploads.pending_mesh_uploads` 的 mesh CPU 数据，在 graphics queue 上完成 vertex/index
  buffer copy 和 BLAS build；mesh 完成前不会被 `RenderInstanceManager` 激活。`SceneChanges.removed_meshes`
  会移除 ready cache，并阻止 late BLAS/geometry completion 重新进入 resolver。
- `RenderMaterialManager` 消费 `WorldRenderSync.scene_changes` 中的 material add/update/remove，维护
  `SceneMaterialHandle -> stable material slot` 映射、FIF material buffer、dirty region 上传、texture ready 检查和延迟
  slot 回收；写 GPU material buffer 时通过 `SceneReadView` 读取 `SceneStore` 的 CPU 权威材质参数。
- `RenderInstanceManager` 消费 `WorldRenderSync.scene_changes` 中的 instance lifecycle / transform 变化，
  同步 `InstanceHandle -> GpuInstanceSlot`，在 mesh/material 都 GPU ready 前保持 pending，并按稳定 slot 输出
  active render list，同时为同步 raycast 生成当前 prepare 快照的 slot 反查表。
- `RayCastService` 持有 runtime 私有的专用 ray tracing pipeline/SBT、可增长 ray/result/readback buffer、
  command pool 和 fence；它由 runtime 拥有，不进入 RenderGraph，也不通过 app 层 pass crate 暴露。
- `SwapchainPresenter` 拥有 surface、swapchain wrapper、swapchain image/view handle 和 present 同步对象；
  app/plugin 只通过 `PresentView` 查询 swapchain 信息，并通过 `ImportedPresentTarget` 接入 RenderGraph，不直接访问 owner 字段或 semaphore。

## 对外接口

- crate 生命周期入口保持在 `present`、`render_runtime_ctx` 和 `render_runtime`；
  app 层相机不属于 runtime 公共 API，prepare 阶段只接收 `RenderView` 快照。
- runtime-owned render state 通过 `state::{frame_state, dlss_options, view_accum, frame_timing, dlss_sr}` 模块公开；
  其中 `dlss_options` 提供 `DlssOptions`，作为 SR/RR active 判断、旧 feature 比较和资源释放的统一 owner；foundation 只保留 FIF 基础索引、资源句柄、view trait 和 `GfxResourceAccess` 契约。
- GPU resource owner 通过 `resources` 模块公开，包括 `GfxResourceManager`、`CmdAllocator` 和 `StageBufferManager`。
- shader-visible binding owner 通过 `bindings` 模块公开，包括 `ShaderBindingSystem`、`GlobalDescriptorSets`、`BindlessManager` 和 `PerFrameGpuData`。
- render-side asset managers、instance manager、`RenderWorld` 数据结构和 prepare 辅助逻辑都是 runtime 私有实现；
  render-side scene owner、resolver trait 和环境绑定快照都收敛在私有 `render_world` 模块。
- 生命周期 Ctx 在 `render_runtime_ctx` 模块定义，并由 `render_runtime` 重新导出；
  调用方仍通过 `truvis_render_runtime::render_runtime::*Ctx` 使用这些阶段契约。
- `RenderRuntimeRenderCtx` 只暴露 `RenderPassRecordCtx`、`RenderSceneView`、`PresentView` 和 timeline；
  不暴露 texture/mesh manager owner，pass 不能绕过 runtime 私有 bridge 读取上传缓存。
- `RenderRuntimeRayCastCtx` 只暴露同步批量 raycast 调用；App 应在 `after_prepare`
  阶段使用它，update/input 阶段不提供该接口。

## 生命周期

- `RenderRuntime::new` 创建与窗口无关的 runtime root state：`Gfx`、`World`、`GfxResourceManager`、
  `ShaderBindingSystem`、`FrameTiming`、`PerFrameGpuData`、runtime render state 和 `RenderWorld`；
  texture/mesh/material/instance/sky/emissive/TLAS owners 在 `RenderWorld::new` 内部初始化。
- `RenderRuntime::init_after_window` 在平台层提供 raw window/display handle 后创建 surface、
  swapchain 与 `SwapchainPresenter`，并返回 init Ctx 供 app/plugin 创建长期 GPU 资源。
- `begin_frame` 是每帧资源回收入口：推进 runtime 私有帧计时器、等待当前 FIF slot、重置 frame command pool、
  清理延迟释放队列，并推进 bindless 与 `RenderWorld` 内部 managers 的 frame token。AssetHub 事件只在
  prepare 边界通过 `World::sync_for_render()` drain。
- `update_phase` 同步 present extent 到 `FrameRenderState`、acquire 当前 swapchain image，并返回 CPU update Ctx。具体窗口尺寸 render target 由 app/plugin 在 init/resize/shutdown 阶段管理。
- App / Plugin update 结束后，`RenderAppShell` 调用 `sync_dlss_options_frame_state`，把 `DlssOptions`
  中的 DLSS SR mode 变化解析为新的 render/output extent；如果 target 尺寸变化，则返回 resize Ctx
  交给 app/plugin 重建自己持有的 RT target、GBuffer 和 main-view target。
- `prepare(render_view)` 是 CPU 语义数据到 GPU 可见数据的边界：它读取 app 提供的 `RenderView`，
  在 `RenderRuntime` 内部同步 material/instance/mesh/texture 状态、上传 RenderWorld
  和 per-frame data，再刷新 per-frame descriptor。
- `ray_cast_phase` 发生在 `prepare` 之后、`render_phase` 之前。同步 raycast 提交到
  graphics queue，并用 fence 阻塞等待 readback；队列顺序保证它能看到本帧 prepare
  提交的 GPU scene/TLAS。
- `render_phase` 返回只读 render Ctx；pass 只能读取 `RenderPassRecordCtx`、`RenderSceneView`、
  present target 和 timeline，不再修改 CPU scene 或接触 manager owner。
- `present` 只提交当前 swapchain image 到 present queue；渲染命令提交由上层 render graph 完成。
- `end_frame` 推进 frame counter，切换下一帧的 FIF label。
- `wait_idle` 在 app/plugin shutdown 前调用，确保上层资源释放时不再被 GPU command 引用。
- `destroy` 等待 GPU idle，依次释放 present、scene/assets、`RenderWorld` 内部 render-side scene resources、
  command allocator、resource manager、sync、sampler、descriptor 等资源，最后销毁 `Gfx`。

## Prepare 数据流

- `RenderRuntime::prepare_render_world` 先调用 `World::sync_for_render()`，把其中的
  `WorldRenderSync.asset_uploads` 交给 `RenderWorld::prepare_asset_sync`，再把
  `WorldRenderSync.scene_changes` 交给 `RenderWorld::prepare_render_data`；`RenderWorld` 内部按 typed payload
  转发给 texture / mesh / material / sky owner。removed texture/mesh/material 会在 asset upload 之前先写入对应
  render manager，避免 stale upload 或 stale slot 在同一帧重新变为 ready。model ready/failed 状态由 `World` 内部的
  `SceneAssetIngestor` 在 asset sync 阶段写回 import status，并自动完成 loader prefab 到 `SceneStore`
  runtime handle 的翻译。
- `RenderRuntime::prepare` 是 update 与 render 之间的固定桥接阶段，按 bindless、`RenderWorld::prepare_render_data`、
  per-frame data 的顺序准备渲染可见数据。
- `RenderMaterialManager` 在 prepare asset sync 中消费 `SceneChanges.changed_materials` / `removed_materials`；
  prepare 阶段再通过 `SceneReadView` 和 `TextureResolver` 把当前 CPU material 参数与 texture fallback/ready 状态按 dirty
  slot 局部写入 material buffer。
- `RenderSkyManager` 在 prepare asset sync 中先同步 `SceneSkyState`，再观察当前 sky texture bytes 并构建 importance
  distribution；在 prepare 阶段通过 `TextureResolver` 查询当前 sky texture 是否 GPU ready。未 ready 或失败时写入纯色
  fallback SRV 与 1x1 fallback distribution，sky revision、真实 sky 切换或 distribution 版本变化时重置累积帧。
- `RenderInstanceManager` 先消费 `SceneChanges` 处理 instance 新增、删除和 transform 变化，再通过
  `World::scene_view()` 暴露的只读 snapshot，结合 `MaterialSlotResolver` 与 `MeshRenderResolver` 做 ready gate；
  material resolver 由 `RenderMaterialManager` 的 scene material stable slot 表提供，只有完整可渲染的实例才进入 `RenderData`。
- `RenderInstanceManager` 在同一次 prepare 输出中同步生成 `GpuInstanceSlot -> CPU record`
  反查快照。raycast readback 只信任这个快照，避免查询阶段重新遍历 CPU scene。
- `RenderWorld` 消费 `RenderData`，按当前 FIF 上传 geometry、instance、light、indirect 和 scene
  root buffer，刷新 raster draw cache，并把 TLAS build / reuse / destroy 委托给内部 `RenderTlasManager`。

## 同步与稳定性约束

- runtime 全局 FIF timeline 确保 frame command pool 与延迟释放资源不会覆盖 GPU 仍在读取的数据。
- texture manager 使用 transfer queue timeline semaphore 异步检测 copy 完成，不阻塞帧循环。
- mesh manager 使用 graphics queue timeline semaphore，因为 BLAS build 不能假设 transfer queue 支持。
- mesh copy 到 BLAS build 前必须覆盖 `TRANSFER_WRITE -> ACCELERATION_STRUCTURE_BUILD_KHR`，
  并包含 device address 输入对应的 `SHADER_READ` 访问。
- material slot 与 instance slot 都延迟到跨过 FIF 窗口后才回收，避免在飞命令中的旧索引指向新对象。
- mesh ready revision 与 instance revision 合成 TLAS revision，`RenderTlasManager` 只在当前 FIF 的 TLAS 过期时重建。
- 同步 raycast 是阻塞接口，适合拾取、编辑器选择等即时交互，不适合作为每帧大规模查询队列。
  结果语义是视觉拾取：closest hit shader 返回可见表面，any-hit 会按材质 opacity / diffuse alpha 忽略透明命中。
- swapchain resize 采用 latest-size 标记；窗口事件只记录最新尺寸，实际重建延迟到 render loop 的安全点。

## Tracy 初始化埋点

- `RenderRuntime::new` 使用一级 span 标记主要初始化阶段，例如 `Gfx`、manager、asset manager、
  RenderWorld、global descriptors、sampler、per-frame buffer 和 command buffer。
- 启动耗时较明显的下层构造函数继续使用二级 span 细分，例如 `RenderTextureManager::new`、
  `RenderSkyManager::new`、`RenderWorld::new`、`GlobalDescriptorSets::new`、`CmdAllocator::new`
  和 `RenderSamplerManager::new`。
- `SceneStore::new` 不在 `truvis-world` 内部添加 Tracy 依赖；它只通过
  `RenderRuntime::new/scene` 这个一级 span 表示。
