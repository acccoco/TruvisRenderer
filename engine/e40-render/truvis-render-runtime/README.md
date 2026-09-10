# truvis-render-runtime

`truvis-render-runtime` 是被 `truvis-render-loop::RenderLoop` 驱动的渲染运行时集成层。
它持有 `Gfx` root owner、CPU `World`、runtime 级 `RenderResourceSystem`、GPU binding/timing owners 和私有的 `RenderWorld`，
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
  与 submesh index 转回 CPU `InstanceHandle` / `MeshHandle` / `MaterialHandle`。
- 提供 `WorldSubmeshSelection` 与只读 selection raster view，把 Renderer 提供的 CPU
  `InstanceHandle + submesh_index` 解析到当前 prepare 快照中的 active raster draw；pending、stale
  或 submesh 越界选择只会跳过绘制，不向上层暴露 GPU slot。
- 负责 surface/swapchain/present image wrapper、acquire/present semaphore 与窗口 resize 重建。
- 不负责窗口事件循环、具体 Renderer/子系统编排、GUI RenderGraph 适配、Assimp 文件导入或具体 pass 逻辑。

## 状态所有权

- `World` 承载 CPU 侧 `SceneStore` 与 `ResourceSystem`，供 update/prepare 阶段读取或修改；Renderer-facing
  model import、procedural mesh/material、runtime instance 和 analytic light 注册通过 `World` facade 进入，
-  render runtime 只通过 `World::poll_asset_loads()` 收敛 loader 状态和 `World::scene_view()`
  只读 scene/resource snapshot 访问这些 CPU owner；scene 语义变化由 prepare 阶段按最终状态对账，不再依赖 change log。
- `GfxResourceManager` 承载 manager-owned GPU image/buffer/view 生命周期。
- `ShaderBindingSystem` 承载 global descriptors、动态 SRV bindless 和 sampler manager，并向 render 阶段提供只读
  shader binding view。全局 set 1 只保存 Material/Scene 数据动态索引的 asset texture 与 sky SRV，固定管线 target
  不进入该表。
- `FrameTiming` 是 runtime-owned 帧状态，统一承载 frame id、FIF label、delta/total time 和可选最小帧间隔；`PerFrameGpuData` 承载 per-FIF `PerFrameData` UBO。
- `FrameRenderState`、`DlssOptions`、`ViewAccumState` 和 `DlssSrState` 定义在本 crate，
  并由 `RenderRuntime` 持有；`DlssOptions` 同时提供 SR/RR active feature 决策。
- runtime 内部拥有默认 120 FPS 软件上限、surface format、present mode 与 depth format 候选顺序；这些默认策略不放入
  foundation 公共配置契约。
- `RenderResourceSystem` 是 runtime 级共享 GPU 资源 owner，内部持有 `RenderTextureManager`、`RenderMeshManager`、
  `RenderMaterialManager`、`RenderSkyManager` 和共享 `RenderAssetUploadQueue`；资源按 CPU handle 发布、
  按 FIF 延迟回收，不依赖某个 instance。
- `RenderWorld` 是 runtime 私有的 scene GPU 翻译层，持有 `RenderInstanceManager`、`RenderAnalyticLightManager`、
  `RenderEmissiveLightTable`、scene/instance/geometry/indirect buffer、raster draw cache 和 `RenderTlasManager`；
  prepare 阶段从 CPU scene 最终状态对账，render pass 只通过 `RenderSceneView` 读取它。
- 默认 sky 通过 `World` facade 注册为普通 `TextureHandle`，再写入 `SceneStore::SceneSkyState`；
  `RenderSkyManager` 从 `World::scene_view()` 读取 sky state，持有常驻纯色 fallback sky、单线程
  `SkyDistributionBuilder`、request generation 与 active/retired distribution。当前 sky CPU texture bytes
  到达后异步构建最高 `4096x2048` 的 Alias 表，再通过共享 transfer timeline 异步上传；
  `RenderWorld` 只消费已发布的 sky 环境绑定快照。
- `RenderTextureManager` 扫描 `SceneReadView` 中已解码的 texture CPU bytes，异步上传 GPU image，并注册
  image view 与 bindless SRV；未 ready 或失败时通过 fallback texture 保证材质仍可安全读取。
  image upload 与 sky distribution buffer upload 共用 `RenderResourceSystem` 私有的 `RenderAssetUploadQueue`；
  默认 sky 的真实 texture 也复用该上传路径，但 sky fallback 由 `RenderSkyManager` 独立维护。CPU registry 中已删除的
  texture 会先移除 ready cache；已提交但未完成的 stale upload 在 timeline 到达后只销毁，不会重新 publish 到 resolver。
- `RenderMeshManager` 扫描 `SceneReadView` 中不可变的 mesh CPU 数据，在 graphics queue 上按 submesh
  创建 vertex/index buffer 和 `RtGeometry`，并为同一个 mesh 构建一个包含多 geometry input 的 BLAS；mesh 完成前不会被 `RenderInstanceManager` 激活。
  CPU registry 中已删除的 mesh 会移除 ready cache，并阻止 late BLAS/geometry completion 重新进入 resolver。
- `RenderMaterialManager` 扫描 CPU material registry，维护 `MaterialHandle -> stable material slot` 映射、FIF
  material buffer、dirty region 上传和延迟 slot 回收；texture binding revision 负责发现异步 ready。
  它在资源对账阶段复制 CPU 材质参数并生成 render-side prepared snapshot，写 GPU material buffer 时只消费这份副本。
- `RenderInstanceManager` 扫描 CPU instance membership，同步 `InstanceHandle -> GpuInstanceSlot`，在 mesh/material 都 GPU ready 前保持 pending，并按稳定 slot 输出
  active render list，同时为同步 raycast 生成当前 prepare 快照的 slot 反查表。每帧 motion history 推进仍属于
  instance manager 自身的 temporal 生命周期维护，不参与 dirty 传播。
- `RenderAnalyticLightManager` 对账 analytic light revision，按 FIF 持有 point / spot / area light structured buffer；
  变化时标记全部 FIF，当前 frame label 使用前上传最新 `SceneReadView` light snapshot，并向 scene root 提供
  device address、count 和 analytic light version。
- `RayCastService` 持有 runtime 私有的专用 ray tracing pipeline/SBT、可增长 ray/result/readback buffer、
  command pool 和 fence；它由 runtime 拥有，不进入 RenderGraph，也不通过 Renderer 层 pass crate 暴露。
- `SwapchainPresenter` 拥有 surface、swapchain wrapper、swapchain image/view handle 和 present 同步对象；
  Renderer/子系统只通过 `PresentView` 查询 swapchain 信息，并通过 `ImportedPresentTarget` 接入 RenderGraph，不直接访问 owner 字段或 semaphore。

## 对外接口

- crate 生命周期入口保持在 `present`、`render_runtime_ctx` 和 `render_runtime`；
  Renderer 层相机不属于 runtime 公共 API，prepare 阶段只接收 `RenderView` 快照。
- runtime-owned render state 通过 `state::{frame_state, dlss_options, view_accum, frame_timing, dlss_sr}` 模块公开；
  其中 `dlss_options` 提供 `DlssOptions`，作为 SR/RR active 判断、旧 feature 比较和资源释放的统一 owner；foundation 只保留 FIF 基础索引、资源句柄、view trait 和 `GfxResourceAccess` 契约。
- GPU resource owner 通过 `resources` 模块公开，包括 `GfxResourceManager`、`CmdAllocator` 和 `StageBufferManager`。
- shader-visible binding owner 通过 `bindings` 模块公开，包括 `ShaderBindingSystem`、`GlobalDescriptorSets`、`BindlessManager` 和 `PerFrameGpuData`。
  `BindlessManager` 只提供 sampled-image SRV 注册与 FIF 延迟回收，不提供 combined-image-sampler 或 storage-image 表。
- render-side asset managers、instance manager、`RenderWorld` 数据结构和 prepare 辅助逻辑都是 runtime 私有实现；
  render-side scene owner、resolver trait 和环境绑定快照都收敛在私有 `render_world` 模块。
- 生命周期 Ctx 在 `render_runtime_ctx` 模块定义，并由 `render_runtime` 重新导出；
  调用方仍通过 `truvis_render_runtime::render_runtime::*Ctx` 使用这些阶段契约。
- `RenderRuntimeRenderCtx` 只暴露 `RenderPassRecordCtx`、`RenderSceneView`、`PresentView` 和 timeline；
  不暴露 texture/mesh manager owner，pass 不能绕过 runtime 私有 bridge 读取上传缓存。
- render 阶段的 `ShaderBindingView` 只暴露 global set layouts/sets；固定管线 image 由各 pass 解析 view handle 并写入
  自己的 local descriptor，不能反查或依赖全局 bindless slot。
- `RenderRuntimeRayCastCtx` 只暴露同步批量 raycast 调用；Renderer 应在 `after_prepare`
  阶段使用它，update/input 阶段不提供该接口。
- `RenderRuntimeRenderCtx` 除普通 `RenderSceneView` 外，还暴露 `WorldSubmeshRasterView` trait
  object，供 Renderer-owned selection outline 等效果以 World 选择语义录制单个 submesh raster draw。

## 生命周期

- `RenderRuntime::new` 创建与窗口无关的 runtime root state：`Gfx`、`World`、`GfxResourceManager`、
  `ShaderBindingSystem`、`FrameTiming`、`PerFrameGpuData`、`RenderResourceSystem` 和 `RenderWorld`；
  texture/mesh/material/sky/upload queue 在 `RenderResourceSystem::new` 初始化，instance/emissive/TLAS
  等场景 owner 在 `RenderWorld::new` 初始化。
- `RenderRuntime::init_after_window` 在平台层提供 raw window/display handle 后创建 surface、
  swapchain 与 `SwapchainPresenter`，并返回 init Ctx 供 Renderer/子系统创建长期 GPU 资源。
- `remaining_until_render` 读取 `FrameTiming` 的最小帧间隔；默认 120 FPS 对应约 `8.333333 ms`。外层 RenderLoop
  在剩余时间大于 1 ms 时短周期 `park_timeout(1 ms)`，最后 1 ms 内有界自旋；重负载超过间隔时不额外等待、不补帧。
- `begin_frame` 是每帧资源回收入口：由 `FrameTiming` 一次采样更新 delta/total time、等待当前 FIF slot、重置 frame command pool、
  清理延迟释放队列，并把当前 frame id 传给 bindless、`RenderResourceSystem` 与 `RenderWorld`；旧 sky distribution
  跨过 FIF 窗口后由 `GfxResourceManager` 销毁。AssetHub 事件只在
  prepare 边界通过 `World::poll_asset_loads()` 收敛。
- `update_phase` 同步 present extent 到 `FrameRenderState`、acquire 当前 swapchain image，并返回 CPU update Ctx。具体窗口尺寸 render target 由 Renderer/子系统在 init/resize/shutdown 阶段管理。
- Renderer update 结束后，`RenderLoop` 调用 `sync_dlss_options_frame_state`，把 `DlssOptions`
  中的 DLSS SR mode 变化解析为新的 render/output extent；如果 target 尺寸变化，则返回 resize Ctx
  交给 Renderer 直接编排各子系统重建自己持有的 RT target、GBuffer 和 main-view target。
- `prepare(render_view)` 是 CPU 语义数据到 GPU 可见数据的边界：它读取 app 提供的 `RenderView`，
  在 `RenderRuntime` 内部同步 material/instance/mesh/texture 状态、上传 RenderWorld
  和 per-frame data，再刷新 per-frame descriptor。
- `ray_cast_phase` 发生在 `prepare` 之后、`render_phase` 之前。同步 raycast 提交到
  graphics queue，并用 fence 阻塞等待 readback；队列顺序保证它能看到本帧 prepare
  提交的 GPU scene/TLAS。
- `render_phase` 返回只读 render Ctx；pass 只能读取 `RenderPassRecordCtx`、`RenderSceneView`、
  present target 和 timeline，不再修改 CPU scene 或接触 manager owner。
- `present` 只提交当前 swapchain image 到 present queue；渲染命令提交由上层 render graph 完成。
- `end_frame` 推进 `FrameTiming` frame id，切换下一帧的 FIF label。
- `wait_idle` 在 Renderer/子系统 shutdown 前调用，确保上层资源释放时不再被 GPU command 引用。
- `destroy` 等待 GPU idle，依次释放 present、CPU scene/assets、`RenderWorld` 场景资源、`RenderResourceSystem` 共享 GPU 资源、
  command allocator、resource manager、sync、sampler、descriptor 等资源，最后销毁 `Gfx`。

## Prepare 数据流

- `RenderRuntime::prepare_render_world` 先调用 `World::poll_asset_loads()`，收敛 loader completion 到 CPU registry；
  `RenderResourceSystem::sync` 按 CPU registry membership 清理 texture/mesh，发布 upload completion，并扫描
  material source revision 与 texture binding revision。removed texture/mesh/material 会在下一次完整对账时
  从对应 render manager 移除，避免 stale upload 或 stale slot。model ready/failed 状态由 `World` 内部的
  `SceneAssetIngestor` 在 asset sync 阶段写回
  import status，并自动完成 loader prefab 到 `SceneStore` runtime handle 的翻译。
- `RenderRuntime::prepare` 是 update 与 render 之间的固定桥接阶段，按 bindless、`RenderWorld::prepare_render_data`、
  per-frame data 的顺序准备渲染可见数据。
- `RenderMaterialManager` 在 prepare asset sync 中扫描 material source revision；scene material 变化会推进 material
  GPU payload 与依赖的 instance 重新计算，texture ready 通过 binding revision 让 fallback 在各 FIF buffer 中被替换。
  资源对账阶段通过 `SceneReadView` 读取 CPU 最终材质状态，准备好材质副本和 texture binding；后续 GPU 上传按
  dirty slot 局部写入 material buffer，不再在上传阶段回读 CPU owner。
- `RenderSkyManager` 在 prepare 中直接对账 `SceneSkyState`，并在资源表提供 texture bytes 后通过
  `Arc` 共享当前 sky texture bytes 给 worker。worker 同时最多一个 in-flight build，连续切换时只保留最新 pending
  request；CPU/GPU completion 必须同时匹配最新 request id 与 `TextureHandle` 才能发布。
  在 prepare 阶段通过 `TextureResolver` 查询当前 sky texture 是否 GPU ready：image ready 而 distribution 未 ready
  时显示真实 HDRI 并使用 1x1 uniform sphere PDF；无效/全黑分布也保持 uniform。sky revision、真实 sky 切换或
  distribution 版本变化时重置累积帧。
- `RenderInstanceManager` 扫描 `World::scene_view()` 暴露的完整 instance membership，比较 transform/material snapshot，
  结合 `MaterialSlotResolver` 与 `MeshRenderResolver` 做 ready gate；
  material resolver 由 `RenderMaterialManager` 的 scene material stable slot 表提供，且 instance material 数量必须与 mesh geometry 数量一致，只有完整可渲染的实例才进入 `RenderData`。
- `RenderInstanceManager` 在同一次 prepare 输出中同步生成 `GpuInstanceSlot -> CPU record`
  反查快照。raycast readback 只信任这个快照，避免查询阶段重新遍历 CPU scene。
- `RenderWorld` 消费 `RenderData`、analytic light binding、environment binding 和 emissive binding，按当前 FIF 上传
  geometry、instance、indirect 和 scene root buffer，刷新 raster draw cache，并把 TLAS build / reuse / destroy 委托给内部
  `RenderTlasManager`。TLAS 和 emissive table 是否重建由本次完整对账的实际结果决定。

## 同步与稳定性约束

- runtime 全局 FIF timeline 确保 frame command pool 与延迟释放资源不会覆盖 GPU 仍在读取的数据。
- `RenderAssetUploadQueue` 使用一套 transfer command pool 与 timeline semaphore，按 FIFO 完成 texture image 和
  sky structured-buffer copy，不阻塞帧循环。sky buffer copy 后建立
  `TRANSFER_WRITE -> SHADER_READ` barrier，timeline 完成前 device address 不写入 scene root。
- 真实天空 Alias distribution 上限为 `4096x2048`、8,388,608 entries、约 128 MiB；
  更高分辨率 HDRI image 保留原尺寸，CPU builder 按源 texel solid angle 向目标 cell 聚合能量。
  active distribution 被替换后按 FIF 延迟释放；未发布的 stale upload 在 transfer 完成后立即销毁。
- lat-long sky sampler 使用 U `REPEAT`、V/W `CLAMP_TO_EDGE`；既有 sampler enum 数值保持不变，新类型追加在末尾。
- mesh manager 使用 graphics queue timeline semaphore，因为 BLAS build 不能假设 transfer queue 支持。
- mesh copy 到 BLAS build 前必须覆盖 `TRANSFER_WRITE -> ACCELERATION_STRUCTURE_BUILD_KHR`，
  并包含 device address 输入对应的 `SHADER_READ` 访问。
- material slot 与 instance slot 都延迟到跨过 FIF 窗口后才回收，避免在飞命令中的旧索引指向新对象。
- 完整对账结果推进 TLAS revision，`RenderTlasManager` 只在当前 FIF 的 TLAS 过期时重建。
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
