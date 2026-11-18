# truvis-render-runtime

`truvis-render-runtime` 是被 `truvis-render-loop::RenderLoop` 驱动的渲染运行时集成层。
它持有 `Gfx` root owner、CPU `GameWorld`、GPU binding/timing owners 和私有的 `RenderWorld`；`RenderWorld` 内部持有 `RenderAssetSystem`，
并通过阶段化的 typed Ctx 向上层暴露初始化、更新、渲染、resize 与 shutdown 能力。

正常关闭先等待 GPU，再停止 sky producer、关闭上传 queue，释放发布资源与子 owner，最后释放 Gfx。
构造和销毁不提供逐层 panic 回滚；真实等待/提交错误按既有致命错误路径传播。
Release 使用 panic=abort，不执行 unwind。Debug 外层线程的 catch、finished 或 join 成功仅说明线程退场，
不能证明异常情况下 Vulkan/SDK 资源已完整释放。回调与 worker 的异常边界由各自 owner 管理。

## 职责边界

- 拥有 `Gfx` root owner，并保证所有 GPU 子资源先于 `Gfx` 销毁。
- 提供 `begin_frame`、`update_phase`、`prepare`、`render_phase`、`present`、`end_frame`、
  `handle_resize`、`shutdown_phase` 和 `destroy` 等生命周期入口。
- 产出 `RenderRuntimeInitCtx`、`RenderRuntimeUpdateCtx`、`RenderRuntimeRenderCtx`、
  `RenderRuntimeRayCastCtx`、`RenderRuntimeResizeCtx` 和 `RenderRuntimeShutdownCtx`，让上层只能在对应阶段访问窄化能力。
- 负责 CPU scene/assets 到 render-side GPU 表示的桥接，包括 texture upload、mesh upload、
  material slot、instance slot、GPU scene buffer、BLAS/TLAS 和 raster draw cache。
- 在 `prepare` 完成后提供 runtime-owned 同步 raycast 服务，把 GPU hit 的 instance slot
  与 submesh index 转回 CPU `MeshInstanceHandle` / `MeshAssetHandle` / `MaterialAssetHandle`。
- 提供 `WorldSubmeshSelection` 与只读 selection raster view，把 Renderer 提供的 CPU
  `MeshInstanceHandle + submesh_index` 解析到当前 prepare 快照中的 active raster draw；pending、stale
  或 submesh 越界选择只会跳过绘制，不向上层暴露 GPU slot。
- 负责 surface/swapchain/present image wrapper、acquire/present semaphore 与窗口 resize 重建。
- 不负责窗口事件循环、具体 Renderer/子系统编排、GUI RenderGraph 适配、Assimp 文件导入或具体 pass 逻辑。

## 状态所有权

共享 texture/sky 上传队列仅在 timeline 查询成功且计数达标后移交资源；查询错误显式传播，
不伪装成计数 0。关闭时先等待最后一笔 pending 提交，成功后才取走 semaphore、command pool
和 pending 资源进行释放；等待失败保留 owner 状态。外层 Runtime 仍负责决定失败退场，
这一局部门槛不构成 device-lost 恢复或跨 queue 可见性的完整证明。

上传准备先检查队列存活和 timeline 容量并预留 pending 槽；完成录制后先移交资源 owner，再调用 native submit。
mesh 与 texture/sky 队列都在 timeline 达标后释放 staging、scratch 和 command，并将完成结果直接交给 consumer。
完成结果与发布通知不承担 panic 后恢复；正常发布仍校验 generational handle，过期资源进入对应释放路径。
关闭已发布资源前，调用方必须确认消费者 GPU 使用结束。

material 每个 FIF 拥有 device/staging buffer 对。资源由显式 destroy 在 GPU 安全点释放；Drop 只检查遗漏释放。

material 注册先确认 slot 容量与诊断，再一起发布 prepared snapshot、source revision、
slot 和变更通知；更新诊断成功后才替换 snapshot。一次对账中断时保留此前已发布的
appearance/emissive 通知，下次成功返回时去重交付，避免 CPU revision 已推进却漏唤醒。

material 到期空 slot 在 begin_frame 的 GPU 安全点回收，早于 sync_scene 的新分配。
更新或删除会撤销旧 payload 的待确认记录；每次 upload 重新建立当前 FIF 的确认集合，
flush/copy/barrier 成功后才登记，queue submit 成功后才清除对应 dirty。该局部状态约束
不提供失败 command 或设备丢失后的 Runtime 恢复策略。

上传 queue 正常关闭等待最后一次提交完成，再释放尚未消费的目标和同步对象。

CPU 完成查询只允许接管上传结果；跨 queue 的可见性由 GPU wait 建立。共享队列保留
已完成的最大 signal，经 sync_assets 返回交给 prepare submit 的 ALL_COMMANDS wait，覆盖同一
graphics queue 后续 raycast/RenderGraph。每帧保留该依赖，不等待尚未完成的上传；关闭
semaphore 前须结束所有引用它的 graphics batches，Runtime 正常 destroy 先等待 device idle。

普通 texture 与 fallback 共用 binding 安装路径。已发布 texture 按 FIF 延迟退役；store 关闭要求消费者停止使用。

RtGeometry 生成 BLAS 输入时核对顶点/索引计数、完整三角形、当前 buffer 容量与地址范围/对齐；
maxVertex 先由数量减一再转换为 u32。CPU 索引值合法性仍由 SubmeshData::validate 保证，输入 buffer
须先完成上传与 transfer-to-build 屏障；这些检查不替代 MeshUploadQueue 的提交和发布生命周期。

mesh copy 在 graphics queue 录制，输入 buffer 的结束屏障同时覆盖 BLAS build、光栅
vertex/index fetch 和 shader device-address 读取；后续消费者仍限定在同一 queue。
共享 image 上传的结束屏障覆盖所有 shader 阶段，跨 queue 部分仍由 prepare 的 wait 补齐。

RayCastPass 拥有 pipeline/layout、descriptor 与 SBT；shader cache 只用于创建阶段。
正常关闭由调用方确认 device idle，再释放 buffer/pass/fence/pool；重复关闭无操作，关闭后禁止查询。
查询 buffer 扩容先创建完整候选并接管字段，再回收旧组合；计数检查 u32 与容量边界。
进入 submit 前服务接管 pending command，fence 等待成功后才解除占用并读回。
submit/wait 错误直接传播；pending command 阻止再次查询，不自动重试。

查询入口在访问/扩容 GPU 资源前检查 ray 数不超过设备的单维派发上限，并拒绝方向长度
平方非有限或过小的输入。合法非单位方向仍由 shader 归一化，t_min/t_max 语义保持不变。

instance table 接管候选后保留到整批最终状态对账完成；查询、版本预检和诊断先于该身份
的镜像变化。已经产生的下游变化标记保留到 RenderData 成功返回，打包中断不会吞掉通知。
slot 回收诊断先于到期项转移，未到期项继续保留；这些局部约束不覆盖调用者后续 prepare 失败。

RenderWorld 持有资源 owner 已交付的去重通知，asset sync 后续失败不丢弃先前结果。
Runtime 在 bindless 准备成功后取走 CPU 候选，并在 RenderWorld 入口移交 table；
table 返回后先登记 scene/TLAS/emissive 失效，再做 analytic 和 scene 上传。历史失效
保留到 prepare submit 确认；这不提供失败 command 的重录或 device-lost 恢复。

Instance transform 采用版本驱动的 dirty 集合与每 FIF current/previous 版本对，仅复制变化矩阵。
每个 FIF 的 SceneBuffers 拥有 scene root 和四组 device/stage buffer。
RenderWorld 正常关闭要求所有场景 GPU 消费结束，依次释放 light、TLAS、FIF buffer，最后关闭共享 assets。

SceneBuffers 扩容在当前 FIF 已等待完成后进行；新 device/stage 完整创建后由字段接管，
再回收旧 pair。扩容后完整上传需求由 owner 保留，后续 pair 或诊断中断不丢失；只有
当前 FIF 的结构上传提交确认才清除，不依赖 scene revision 恰好同时变化。

结构扩容前由 RenderData 验证紧凑 mesh 前缀、唯一递增的稳定 slot、mesh/材质并行数组
及 GPU uint 索引范围，并复用所得数量扩容。宿主侧排他上界不写入 shader，最后一个
有效索引可为 u32::MAX；顶点数在 Geometry 写入时单独检查。非法派生数据触发断言，
不提供静默截断或兼容修正。geometry 和两张 indirect 表只复制已写入的有效前缀，
空表跳过复制；预分配容量不代表 shader 可访问的有效范围。

prepare 上传提交确认副本版本；Loop 在 Renderer 返回后调用 `finish_rendered_frame`，仅主视图
实际提交时推进运动历史。稳定后退出 dirty；结构上传和容量重建复用同一矩阵写入路径。

矩阵准备返回当前 FIF 的具体结果，stage 写入/诊断失败不修改副本确认或历史；调用方
完整录制 copy/barrier 后才移交 table。已录制未确认时拒绝重入；commit 和主视图历史
确认均先校验整批身份/版本，再统一推进。scene pending revision 在 TLAS/root 录制成功
后登记；这里不承担部分 command 的 reset 或未知提交的恢复。

SceneTlas、analytic 和 emissive 表只在所有相关 GPU 使用结束后释放。
表扩容先创建完整候选再交接字段、回收旧 frame；新地址的完整上传需求保留至提交确认。

emissive base map 必须与 geometry indirect 同序同长；Active 材质和 geometry metadata
缺失属于契约错误。发光 submesh 的退化三角形也保留 record，primitive 编号连续；alias
仅引用可采样 record，不能用 alias_count 代替 record_count 判界。
每类 analytic 表从 512 初始容量按需扩容；完整 pair 先接管再回收旧资源，dirty 到提交
确认才清除。上传只复制有效前缀，空类别以 binding count=0 表达，不访问预分配尾部。
selection 与同步 raycast 只查询当前 prepare 快照，
调用方必须使用当前帧上下文的 label；slot 反查表不提供旧 FIF 的历史查询。

- `GameWorld` 承载 CPU 侧 `SceneStore` 与 `AssetSystem`，供 update/prepare 阶段读取或修改；Renderer-facing
  model import、procedural mesh/material、runtime instance 和 analytic light 注册通过 `GameWorld` facade 进入，
-  render runtime 只通过 `GameWorld::poll_asset_loads()` 收敛 loader 状态和 `GameWorld::scene_view()`
  只读 scene/resource snapshot 访问这些 CPU owner；scene 语义变化由 prepare 阶段按最终状态对账，不再依赖 change log。
- `GfxResourceRegistry` 承载 manager-owned GPU image/buffer/view 生命周期。
  延迟释放每个有效 handle 只保留最新退役帧，null/stale 请求不入队；重复请求不能提前释放。
  cleanup 由 begin_frame 在 FIF timeline 等待后调用；registry 不自行同步 GPU，调用方须先停止
  后续发布/使用。立即释放仍要求已证明 GPU 不再引用；image 回收包含关联的所有 views。
  registry 保证 view-before-image；destroy_all 后再登记会重新启用遗漏释放诊断。

- `ShaderBindingSystem` 承载 global descriptors、动态 SRV bindless 和 sampler manager，并向 render 阶段提供只读
  shader binding view。全局 set 1 只保存 Material/Scene 数据动态索引的 asset texture 与 sky SRV，固定管线 target
  不进入该表。
  GlobalDescriptorSets 使用显式 device context；set 随 pool 回收，sampler 由 manager 拥有。销毁要求 GPU 使用结束。

- `FrameTiming` 是 runtime-owned 帧状态，统一承载 frame id、FIF label、delta/total time 和可选最小帧间隔；`PerFrameGpuData` 承载 per-FIF `PerFrameData` UBO。
  FIF UBO 与 command pool 由各自 owner 显式释放。
  `CmdAllocator` 在 timeline 安全点 reset pool 并保留 command handle；显式 free 要求 GPU 已完成且外部 clone 已撤销。

- `FrameRenderState`、`FrameTiming`、FIF、per-frame UBO 和 history invalidation 由本 crate 持有；
  DLSS、ViewAccum 和具体 Renderer temporal state 不属于 Runtime。
- runtime 内部拥有默认 120 FPS 软件上限、surface format、present mode 与 depth format 候选顺序；这些默认策略不放入
  foundation 公共配置契约。
- `RenderWorld` 是 runtime 私有的 scene GPU 翻译层，内部持有 `RenderAssetSystem`、`RenderInstanceTable`、
  `AnalyticLightTable`、`RenderEmissiveLightTable`、scene/instance/geometry/indirect buffer、raster draw cache
  和 `SceneTlas`；同一个 RenderWorld 内的多个 instance 共享资源 manager，render pass 只通过 `RenderSceneView` 读取它。
- `RenderAssetSystem` 持有 `GpuTextureStore`、`GpuMeshStore`、`GpuMaterialStore`、`GpuSkyStore` 和
  `GpuAssetUploadQueue`；资源按 CPU handle 发布、按 FIF 延迟回收，不依赖某个 instance。跨多个 RenderWorld
  的 device-level 资源共享不属于当前实现范围。
- 默认 sky 通过 `GameWorld` facade 注册为普通 `TextureAssetHandle`，再写入 `SceneStore::SceneSkyState`；
  `GpuSkyStore` 从 `GameWorld::scene_view()` 读取 sky state，持有常驻纯色 fallback sky、单线程
  `SkyDistributionBuilder`、request generation 与 active/retired distribution。当前 sky CPU texture bytes
  到达后异步构建最高 `4096x2048` 的 Alias 表，再通过共享 transfer timeline 异步上传；
  `RenderWorld` 只消费已发布的 sky 环境绑定快照。
  Builder 用三行缓存对源线性亮度做横纵 `[1,6,1]/8` 预过滤（U 环绕、V 钳制），
  然后乘源立体角累加 cell 权重；额外内存为 O(源宽度)。该近似 proposal 覆盖双线性
  天空的高亮边缘，不修改实际 HDRI。Alias 与 PDF 共享过滤权重，全黑输入回退 uniform，
  有限正能量不按绝对 epsilon 剔除。request generation、上传和 retired buffer 生命周期保持不变。
- `GpuTextureStore` 扫描 `SceneReadView` 中已解码的 texture CPU bytes，异步上传 GPU image，并注册
  每个 handle 唯一的同格式 image view 与 bindless SRV（RGBA8 按源用途使用 sRGB 或 UNORM，HDR/EXR 使用 SFLOAT）；
  未 ready 或失败时通过 fallback texture 保证材质仍可安全读取。
  image upload 与 sky distribution buffer upload 共用 `RenderAssetSystem` 私有的 `GpuAssetUploadQueue`；
  默认 sky 的真实 texture 也复用该上传路径，但 sky fallback 由 `GpuSkyStore` 独立维护。CPU registry 中已删除的
  texture 会先移除 ready cache；已提交但未完成的 stale upload 在 timeline 到达后只销毁，不会重新 publish 到 resolver。
- `GpuMeshStore` 扫描 `SceneReadView` 中不可变的 mesh CPU 数据，在 graphics queue 上按 submesh
  创建 vertex/index buffer 和 `RtGeometry`，并为同一个 mesh 构建一个包含多 geometry input 的 BLAS；mesh 完成前不会被 `RenderInstanceTable` 激活。
  CPU registry 中已删除的 mesh 会移除 ready cache，并阻止 late BLAS/geometry completion 重新进入 resolver。
- `GpuMaterialStore` 扫描 CPU material registry，维护 `MaterialAssetHandle -> stable material slot` 映射、FIF
  material buffer、dirty region 上传和延迟 slot 回收；texture binding revision 负责发现异步 ready。
  它在资源对账阶段复制 CPU 材质参数并生成 render-side prepared snapshot，写 GPU material buffer 时只消费这份副本。
- `RenderInstanceTable` 首次初始化完整 membership，后续仅对账 CPU 编辑与资源反向引用合并的候选 handle；同步 `MeshInstanceHandle -> GpuInstanceSlot`，在 mesh/material 都 GPU ready 前保持 pending，并按稳定 slot 输出
  active render list，同时为同步 raycast 生成当前 prepare 快照的 slot 反查表。运动历史与 FIF 版本
  由 instance manager 的 dirty 集合收敛，不向 GameWorld 反向传播 previous 更新。
- instance slot 不再预设固定数量；新 slot 按稳定索引增长，删除后仍跨 FIF 窗口复用。当前 FIF 的
  geometry、instance 和 indirect buffer 在 prepare 时按实际数据以二次幂容量扩容，扩容后整表重传。
- bindless sampled-image descriptor 上限为 1024，descriptor pool 与生成的 layout count 使用同一来源；
  material slot 仍由 `GpuMaterialStore` 的独立容量策略管理。
  到期 bindless slot 在 begin_frame 的 FIF 等待之后、纹理注册之前归还；prepare 只提交新增
  descriptor 写入，写入失败保留 dirty。null view 在修改状态前拒绝，shader index 转换检查整数范围；
  live slot 不原地覆盖，调用方仍须先撤销后续 GPU 引用再 unregister。
- `AnalyticLightTable` 对账 analytic light revision，按 FIF 持有 point / spot / area light structured buffer；
  变化时标记全部 FIF，当前 frame label 使用前上传最新 `SceneReadView` light snapshot，并向 scene root 提供
  device address、count 和 analytic light version。
- `RayCastService` 持有 runtime 私有的专用 ray tracing pipeline/SBT、可增长 ray/result/readback buffer、
  command pool 和 fence；它由 runtime 拥有，不进入 RenderGraph，也不通过 Renderer 层 pass crate 暴露。
- `SwapchainPresenter` 拥有 surface、swapchain wrapper、swapchain image/view handle 和 present 同步对象；
  Renderer/子系统只通过 `PresentView` 查询 swapchain 信息，并通过 `ImportedPresentTarget` 接入 RenderGraph，不直接访问 owner 字段或 semaphore。
  窗口初始化拒绝覆盖已有 Presenter；构造时逐项持有新资源，未提交阶段失败显式回收。
  创建 views 前先记录 registry image handle，helper 失败撤销本次登记集合。该回收仍依赖 registry
  释放接口正常返回，不承诺 logger 再次 panic 或宿主分配 abort 时的完整退出。
  重建要求没有在手 image；等待失败不修改旧字段，新链交付失败时清空旧 view 引用并回收新链，
  继续传播错误。当前重建仍未按新 image 数重建 present 等待 semaphore，也没有旧代 presentation
  完成凭据；不能将 device idle 等同于整个 WSI 退场完成。

## 对外接口

- crate 生命周期入口保持在 `present`、`render_runtime_ctx` 和 `render_runtime`；
  Renderer 层相机不属于 runtime 公共 API，prepare 阶段只接收 `RenderFrameInput` 快照。
- runtime-owned render state 通过 `state::{frame_state, frame_timing}` 模块公开；DLSS 类型在 Renderer pass 层，
  `TruvisRenderer` 负责其配置、能力、资源和 temporal state。
- GPU resource owner 通过 `resources` 模块公开，包括 `GfxResourceRegistry` 和 `CmdAllocator`；staging 由实际上传 owner 持有并按对应提交的完成条件回收。
- shader-visible binding owner 通过 `bindings` 模块公开，包括 `ShaderBindingSystem`、`GlobalDescriptorSets`、`BindlessManager` 和 `PerFrameGpuData`。
  `BindlessManager` 只提供 sampled-image SRV 注册与 FIF 延迟回收，不提供 combined-image-sampler 或 storage-image 表。
- render-side asset managers、instance manager、`RenderWorld` 数据结构和 prepare 辅助逻辑都是 runtime 私有实现；
  render-side scene owner、resolver trait 和环境绑定快照都收敛在私有 `render_world` 模块。
- 生命周期 Ctx 在 `render_runtime_ctx` 模块定义，并由 `render_runtime` 重新导出；
  调用方仍通过 `truvis_render_runtime::render_runtime::*Ctx` 使用这些阶段契约。
- `RenderRuntimeRenderCtx` 暴露 `RenderPassRecordCtx`、scene/selection/present 只读视图、timeline
  和 device/resource/queue/设备查询能力；不暴露 texture/mesh manager owner，pass 不能绕过
  runtime 私有 bridge 读取上传缓存。只读 scene 不等于禁止调用底层 GPU 分配或提交接口。
- render 阶段的 `ShaderBindingView` 只暴露 global set layouts/sets；固定管线 image 由各 pass 解析 view handle 并写入
  自己的 local descriptor，不能反查或依赖全局 bindless slot。
- `RenderRuntimeRayCastCtx` 只暴露同步批量 raycast 调用；Renderer 应在 `after_prepare`
  阶段使用它，update/input 阶段不提供该接口。
- `RenderRuntimeRenderCtx` 除普通 `RenderSceneView` 外，还暴露 `WorldSubmeshRasterView` trait
  object，供 Renderer-owned selection outline 等效果以 GameWorld 选择语义录制单个 submesh raster draw。

## 生命周期

- `RenderRuntime::new` 创建与窗口无关的 runtime root state：`Gfx`、`GameWorld`、`GfxResourceRegistry`、
  `ShaderBindingSystem`、`FrameTiming`、`PerFrameGpuData` 和 `RenderWorld`；texture/mesh/material/sky/upload queue
  与 instance/emissive/TLAS 等场景 owner 都在 `RenderWorld::new` 中初始化，前者由其内部的
  `RenderAssetSystem` 持有。
- `RenderRuntime::init_after_window` 在平台层提供 raw window/display handle 后创建 surface、
  swapchain 与 `SwapchainPresenter`，并返回 init Ctx 供 Renderer/子系统创建长期 GPU 资源。
- `remaining_until_render` 读取 `FrameTiming` 的最小帧间隔；默认 120 FPS 对应约 `8.333333 ms`。外层 RenderLoop
  在剩余时间大于 1 ms 时短周期 `park_timeout(1 ms)`，最后 1 ms 内有界自旋；重负载超过间隔时不额外等待、不补帧。
- `begin_frame` 是每帧资源回收入口：由 `FrameTiming` 一次采样更新 delta/total time、等待当前 FIF slot、重置 frame command pool、
  清理延迟释放队列，并把当前 frame id 传给 bindless 与 `RenderWorld` 内部的资源/场景 manager；旧 sky distribution
  跨过 FIF 窗口后由 `GfxResourceRegistry` 销毁。AssetLoadService 事件只在
  prepare 边界通过 `GameWorld::poll_asset_loads()` 收敛。
- `update_phase` 同步 present extent 到 `FrameRenderState`、acquire 当前 swapchain image，并返回 CPU update Ctx。具体窗口尺寸 render target 由 Renderer/子系统在 init/resize/shutdown 阶段管理。
- Renderer update 结束后，`RenderLoop` 调用 `sync_render_extent`，把 Renderer 提交的中性内部尺寸请求
  应用到 `FrameRenderState`；如果 target 尺寸变化，则返回 resize Ctx 交给 Renderer 编排子系统重建。
- `prepare(RenderFrameInput)` 是 CPU 语义数据到 GPU 可见数据的边界：它读取 Renderer 提供的当前/上一帧视图与 jitter，
  通过 `RenderWorld` 内部同步 material/instance/mesh/texture 状态、上传 RenderWorld
  和 per-frame data，再刷新 per-frame descriptor。
- `ray_cast_phase` 发生在 `prepare` 之后、`render_phase` 之前。同步 raycast 提交到
  graphics queue，并用 fence 阻塞等待 readback；队列顺序保证它能看到本帧 prepare
  提交的 GPU scene/TLAS。
- `render_phase` 返回只读 render Ctx；pass 只能读取 `RenderPassRecordCtx`、`RenderSceneView`、
  present target 和 timeline，不再修改 CPU scene 或接触 manager owner。
- `present` 只提交当前 swapchain image 到 present queue；渲染命令提交由上层 render graph 完成。
- `end_frame` 在 present 或空 signal 提交正常返回后推进 `FrameTiming` frame id，切换
  下一帧 FIF label；不表示 GPU 已完成。frame id 耗尽时明确失败，不回绕到旧 timeline 序号。
- `wait_idle` 在 Renderer/子系统 shutdown 前调用，确保上层资源释放时不再被 GPU command 引用。
- `destroy` 等待 GPU idle，依次释放 present、CPU scene/assets、`RenderWorld` 场景资源与其内部的 `RenderAssetSystem`、
  command allocator、resource manager、sync、sampler、descriptor 等资源，最后销毁 `Gfx`。

## Prepare 数据流

- `RenderRuntime::prepare_render_world` 先调用 `GameWorld::poll_asset_loads()`，收敛 loader completion 到 CPU registry；
  `RenderWorld::sync_assets` 通过内部 `RenderAssetSystem::sync` 按 CPU registry membership 清理 texture/mesh，发布 upload completion，并扫描
  material source revision 与 texture binding revision。removed texture/mesh/material 会在下一次完整对账时
  从对应 render manager 移除，避免 stale upload 或 stale slot。model ready/failed 状态由 `GameWorld` 内部的
  `AssetSystem` 在 asset sync 阶段写回
  import status 并发布 `SceneData`；外部 Renderer 根据 scene data 显式完成到 `SceneStore`
  runtime handle 的翻译。
- `RenderRuntime::prepare` 是 update 与 render 之间的固定桥接阶段，按 bindless、`RenderWorld::prepare_render_data`、
  per-frame data 的顺序准备渲染可见数据。
- `GpuMaterialStore` 在 prepare asset sync 中扫描 material source revision；scene material 变化会推进 material
  GPU payload 与依赖的 instance 重新计算，texture ready 通过 binding revision 让 fallback 在各 FIF buffer 中被替换。
  资源对账阶段通过 `SceneReadView` 读取 CPU 最终材质状态，准备好材质副本和 texture binding；后续 GPU 上传按
  dirty slot 局部写入 material buffer，不再在上传阶段回读 CPU owner。
- `GpuSkyStore` 在 prepare 中直接对账 `SceneSkyState`，并在资源表提供 texture bytes 后通过
  `Arc` 共享当前 sky texture bytes 给 worker。worker 同时最多一个 in-flight build，连续切换时只保留最新 pending
  request；CPU/GPU completion 必须同时匹配最新 request id 与 `TextureAssetHandle` 才能发布。
  请求开始时记录 generation 并退役旧分布；builder 派发失败或断连后交付失败并永久不可用，
  不自动重启。CPU completion 由 poll 转交 store，可恢复准备失败通知当前请求使用 uniform，
  native submit 错误仍走致命路径，不提供逐层 unwind 回滚。
  shutdown 丢弃 pending，等待当前 build 结束，join 结果由 builder 记录；未消费 CPU payload
  在 join 后释放。随后在 GPU 安全点依次回收分布与 fallback。
  在 prepare 阶段通过 `TextureResolver` 查询当前 sky texture 是否 GPU ready：image ready 而 distribution 未 ready
  时显示真实 HDRI 并使用 1x1 uniform sphere PDF；无效/全黑分布也保持 uniform。sky revision、真实 sky 切换或
  distribution 版本变化时重置累积帧；binding 解析与诊断成功后才确认已交付的变化，
  这一确认只覆盖本次 binding 返回，不代表外层 prepare 已完成。
- `RenderInstanceTable` 首次扫描完整 membership，后续读取已接管候选的 CPU 最终状态，比较 transform/material snapshot，
  结合 `MaterialSlotResolver` 与 `MeshRenderResolver` 做 ready gate；
  material resolver 由 `GpuMaterialStore` 的 scene material stable slot 表提供，且 instance material 数量必须与 mesh geometry 数量一致，只有完整可渲染的实例才进入 `RenderData`。
- `RenderInstanceTable` 在同一次 prepare 输出中同步生成 `GpuInstanceSlot -> CPU record`
  反查快照。raycast readback 只信任这个快照，避免查询阶段重新遍历 CPU scene。
- `RenderWorld` 消费 `RenderData`、analytic light binding、environment binding 和 emissive binding，按当前 FIF 上传
  geometry、instance、indirect 和 scene root buffer，刷新 raster draw cache，并把 TLAS build / reuse / destroy 委托给内部
  `SceneTlas`。TLAS 和 emissive table 是否重建由本次完整对账的实际结果决定。

## 同步与稳定性约束

- runtime 全局 FIF timeline 确保 frame command pool 与延迟释放资源不会覆盖 GPU 仍在读取的数据。
- `GpuAssetUploadQueue` 使用一套 transfer command pool 与 timeline semaphore，按 FIFO 完成 texture image 和
  sky structured-buffer copy，不阻塞帧循环。sky buffer copy 后建立
  `TRANSFER_WRITE -> SHADER_READ` barrier，timeline 完成前 device address 不写入 scene root。
- 真实天空 Alias distribution 上限为 `4096x2048`、8,388,608 entries、约 128 MiB；
  更高分辨率 HDRI image 保留原尺寸，CPU builder 按源 texel solid angle 向目标 cell 聚合能量。
  active distribution 被替换后按 FIF 延迟释放；未发布的 stale upload 在 transfer 完成后立即销毁。
- lat-long sky sampler 使用 U `REPEAT`、V/W `CLAMP_TO_EDGE`；既有 sampler enum 数值保持不变，新类型追加在末尾。
- mesh manager 使用 graphics queue timeline semaphore，因为 BLAS build 不能假设 transfer queue 支持。
- mesh timeline 查询错误沿用 RHI fail-fast 契约，不能当作未完成；关闭时先等待成功再移出
  semaphore/pool，等待失败保留 pending 的 command、staging、scratch、geometry 与 BLAS owner。
  保留 owner 不等于设备可恢复，外层 Runtime 仍负责失败退场。
- mesh copy 到 BLAS build 前必须覆盖 `TRANSFER_WRITE -> ACCELERATION_STRUCTURE_BUILD_KHR`，
  并包含 device address 输入对应的 `SHADER_READ` 访问。
- material slot 与 instance slot 都延迟到跨过 FIF 窗口后才回收，避免在飞命令中的旧索引指向新对象。
- 完整对账结果推进 TLAS revision，`SceneTlas` 只在当前 FIF 的 TLAS 过期时重建。
- 同步 raycast 是阻塞接口，适合拾取、编辑器选择等即时交互，不适合作为每帧大规模查询队列。
  结果语义是视觉拾取：closest hit shader 返回可见表面，any-hit 会按材质 opacity / diffuse alpha 忽略透明命中。
- swapchain resize 采用 latest-size 标记；窗口事件只记录最新尺寸，实际重建延迟到 render loop 的安全点。

## Tracy 初始化埋点

- `RenderRuntime::new` 用嵌套 span 分别记录 Gfx、frame state、timeline、registry、command
  allocator、bindings、GameWorld、默认天空注册、RenderWorld、raycast、per-frame buffer
  和 command buffer 初始化。span 不持有资源，也不承担回滚。
- 下层构造函数保留自身 span；`GameWorld`/`SceneStore` 不因此增加 Tracy 依赖，CPU scene
  创建时间由 `RenderRuntime::new/world` 观测。

Sky builder 对大型工作集使用 `try_reserve_exact`，失败结果携带 request id、texture 和诊断。
零能量仍返回 UniformFallback；构建失败仅影响对应 generation，过期失败不能覆盖新天空。
通道断连或派发失败后 builder 永久不可用，清空运行中与待派发请求并交付失败，不自动重启。
消费者收到当前失败后通知 scene 使用 uniform sphere PDF；每次失败记录一次，永久不可用仅记录一次。
shutdown 仍 join 并丢弃未消费 CPU payload。unwind catch 仅补充 Debug 等 unwind 构建，Release panic=abort 无恢复保证。
