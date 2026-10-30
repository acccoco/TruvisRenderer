# truvis-gfx

Vulkan RHI 抽象层，封装设备、队列、资源、同步与图形/计算/光追管线基础能力。

## 主要内容

- `Gfx` 显式 root owner 与 typed Gfx Ctx 工厂
- 命令缓冲、同步原语、屏障封装
- 图像/缓冲等 GPU 资源对象
- `GfxSBTBuffer` 负责 ray tracing SBT buffer、region 对齐和 shader group handle 写入
- 交换链与渲染目标底层支持

## Root owner

- `Gfx` 是 Vulkan root owner，由上层显式创建并持有；生产代码不通过全局 singleton 访问 GPU 设备。
- 默认 `Gfx::new(...)` 同时拥有 Streamline runtime 生命周期：先执行 `slInit`，再通过
  `sl.interposer.dll` 创建 Vulkan entry，最后在 `Gfx::destroy(...)` 释放内部 device child 后、
  销毁 Vulkan device/instance 前执行 `slShutdown`。
- `Gfx::new_with_entry_source(...)` 是测试或特殊启动路径使用的显式 Vulkan loader 注入入口；
  它只按传入的 `VulkanEntrySource` 创建 Vulkan root，不自动初始化或持有 Streamline runtime；
  特殊路径如需 Streamline，须在外层自行保证 `slInit -> Vulkan root -> child resources -> slShutdown`。
- `Gfx` 负责创建 typed Ctx。所有 child resources 显式销毁后，`Gfx::destroy(...)` 按顺序释放内部
  command pool / allocator，记录 Streamline 关闭日志并执行 `slShutdown`，再 `GfxCore::destroy`；
  `GfxCore` 内部按 messenger、device、instance 顺序销毁，
  `vk_entry` 为最后一个字段，保证 loader 函数表在所有 wrapper 之后 drop。
- 上层 owner 负责安排销毁顺序，`Gfx` 不隐藏清理仍被外部持有的资源。
- 构造是顺序执行：Entry → Instance → PhysicalDevice → Device → queue/messenger → command pool →
  allocator。构造期间任一步失败直接 panic；release profile 为 `panic = 'abort'`，不提供构造回滚。
- 当前主队列家族必须同时支持 graphics/compute 且至少提供两个 queue；不会把只匹配任一能力的
  家族当成主队列。graphics/compute 已隐含 transfer 支持，不要求驱动额外报告 TRANSFER bit。
  专用 compute/transfer 家族仍可缺失；这些筛选不等同于完整的 device extension/feature 能力协商。

## Native Vulkan 命令

- `GfxDevice.push_descriptor` 持有 `truvis_vk_binding::Device`；它只保存与当前设备配对的 POD 函数表。
- `vkCmdPushDescriptorSetKHR` 由 native C API 通过 ash Instance 提供的 `vkGetDeviceProcAddr` 加载并同步调用；
  DLL 来源继续由 `VulkanEntrySource` 决定。必要命令缺失时构造直接 panic。
  binding 构造是 unsafe；同一 Instance 刚创建 Device、尚未发布时完成查询并承担有效性前置。
- `GfxCommandBuffer::push_descriptor_set` 在 `GfxWriteDescriptorSet::with_writes` 闭包内传递 ash 数组，
  包括加速结构的 `pNext`；native 不保存 CPU 指针，GPU 对象的有效期继续由原资源 owner 保证。
  空 push 列表由 binding 在进入 native 前拒绝；通用 with_writes 保留普通 descriptor update 的使用语义。
- wrapper 不负责 Vulkan release；最后一次命令调用必须早于 Device / Instance / Entry 的销毁。
  详细 ABI 契约见 [`truvis-vk-binding`](../../../cxx/rust/bindings/truvis-vk-binding/README.md)。

## Descriptor 声明与写入

- `DescriptorBinding` derive 检查字段属性、唯一 binding 编号和 getter 名称；它不反射 shader，Rust layout
  仍须与 Slang owner 的 set/binding/type/count/stage 对照。类型关联不等于完整 Vulkan 合法性验证。
- `GfxDescriptorCursor` 仅写对应 binding，检查非空 payload、descriptor 类型与数组容量；不使用 Vulkan
  的跨连续 binding 更新语义。buffer、image/sampler、KHR acceleration structure 是当前支持的三类 payload。
- `GfxWriteDescriptorSet::with_writes` 再次检查唯一 payload 与类型，按实际数组长度生成数量；加速结构的
  descriptorCount 与 pNext 数量相同。基础 Vulkan 投影是私有步骤，避免漏掉 pNext 后直接使用。
- pNext、image/buffer info 和 handle 数组只在同步回调内有效；这些检查不证明 handle 有效、image layout
  正确或 GPU 使用已结束。资源 owner 仍负责对象有效期、同步和 descriptor set 身份。

## Ctx 规则
- `Gfx::new(...)` 返回由调用方持有的 root owner；生产代码不通过全局 singleton 访问 `Gfx`。
- `GfxDeviceCtx`、`GfxResourceCtx`、`GfxQueueCtx`、`GfxSurfaceCtx`、`GfxDeviceInfoCtx`、`GfxImmediateCtx` 只暴露当前能力需要的 Vulkan 依赖。
- 上层创建、提交、查询或销毁资源时，应传入最窄的 typed Ctx，避免把完整 `&Gfx` 或长期引用存进资源对象。
- `GfxDeviceInfoCtx::max_ray_dispatch_width` 从已缓存属性计算 `[width, 1, 1]` 光追派发上限，
  同时约束 X 维度和总 invocation 数；多维派发仍须检查各维与总乘积，不能直接套用该单维上限。

## Vulkan 诊断回调

debug messenger 的固定 flags 按值构造，不使用全局可变初始化。回调保持同步日志与现有 JSON
MainMessage 展开方式；logger 和 panic hook 不得重入 Vulkan。可捕获的 Rust panic 在 ABI 内隔离，
不再次记录错误，仍返回 `VK_FALSE`；panic payload 被保留，避免其析构再次 panic。该隔离只因 unwind
不能穿越 `extern "system"` FFI 边界而存在，不覆盖 panic=abort、OOM abort 或无效 native 指针。

## 生命周期规则

主机映射写入通过 `&mut GfxBuffer` 完成；上传数据必须满足 `NoUninit`，类型化引用必须满足
`Pod`，并在每次访问检查实际容量、host 地址范围和对齐。新映射的字节先置零，避免把 VMA
未初始化内存直接解释为 Rust 引用；这是创建时的 CPU 初始化成本，不代表已向 GPU flush。
structured buffer 的元素计数不能代替当前 inner 分配的容量检查。空上传不分配、不提交；
callback staging 上传在提交前统一 flush，raw pointer 写入者仍承担字节范围与初始化责任。
index buffer 的便捷构造接收同一 `T` 的 slice，分配前检查 index count 乘元素宽度是否溢出，
避免 u16 分配接受 u32 上传；零容量资源不创建。
这些约束不证明同 device/allocator、GPU 完成或 cache 可见性：owner 必须等待安全点，读回前
invalidate，写入后 flush，再安排显式销毁。

`GfxImageView::image_handle` 保留创建时的借用来源，供上层核对 image/view 配对，不拥有 image。
`GfxImageBarrier::whole_image` 显式覆盖全部 mip/layer，要求这些子资源使用共同的旧/新 layout；
未调用时仍保留单 mip/layer 默认范围。

图像 view 的 mip/layer 范围完整投影到 native create info，计数必须非零；实际图像是否包含这些
子资源仍由 owner 保证。2D 图像尺寸非零，concurrent sharing 只接受至少两个不同 queue family，
family 是否属于当前设备仍由调用方保证。紧密排列的 image 上传在分配/录制前验证宿主字节数，
仅覆盖 mip 0/layer 0，结束屏障覆盖同一 queue 的所有 shader 读取；跨 queue 消费仍需 semaphore
wait，不能由 CPU 完成查询替代，也不能替代 RenderGraph 的布局/访问依赖分析。

- Vulkan/VMA/WSI wrapper 只通过显式 `destroy(...ctx...)` / `destroy_mut(...ctx...)` 释放底层对象。
- `Drop` 只做 debug assertion，用于暴露遗漏的显式销毁，不再调用 Vulkan/VMA/WSI release API。
  `VMemAllocator` 用 ManuallyDrop 阻止 inner 的隐式 VMA 释放，只有显式 destroy 取出并释放一次。
  遗漏 destroy 在 Debug 报错，在无断言构建中泄漏，不能依靠不确定的 Drop 顺序回收 native。
- 失败策略：构造和同步上传均为顺序执行，任一步失败直接 panic，不做 catch_unwind 回滚或
  best-effort 清理。release profile 为 `panic = 'abort'`，进程终止时由驱动回收 native 资源；
  唯一的 `catch_unwind` 位于 Vulkan debug callback 的 FFI 边界。
- Streamline 由 `Gfx` 显式包住 Vulkan root 生命周期；调用方不应在 `RenderRuntime` 或更上层指定
  Streamline plugin 路径、日志路径或 Vulkan loader 路径，也不应直接决定 SL 开关。
- `GfxBuffer`、`GfxImage`、`GfxImageView`、swapchain/surface、pipeline、descriptor、sampler、query、sync 等对象的销毁依赖必须由 owner 在调用点传入。
- swapchain 创建前必须查询 surface 支持的 formats 与 present modes；项目默认格式和 present mode 是硬性要求，不支持时启动阶段直接失败。
- WSI 创建同时检查 graphics family 的 present 支持、非零 extent、image 数量、usage、layer 和 OPAQUE
  composite alpha。替换接口消费旧链：native create 后立即销毁旧 swapchain，调用方必须先完成旧链
  GPU/WSI 使用并释放 views/wrappers。
  external image wrapper 接收 owner 提供的真实 usage/flags，不能把 WSI image 登记成无用途的 image。
- swapchain wrapper 只允许一张已 acquire、尚未 present 的 image；current image getter 只在持有时可用。
  acquire 至少提供 semaphore/fence，WSI semaphore 必须为 binary，present 使用 graphics family。
  TIMEOUT/NOT_READY 返回 NotReady，不产生 image 或 resize 请求；present 成功/suboptimal/out-of-date
  后消耗当前 acquisition。同步对象存活、信号/等待完成及旧代 presentation 退场仍由 owner 保证。
- `GfxFence` / `GfxSemaphore` 是唯一 owner，不可 clone；queue submit 只借用 Fence，等待和销毁
  仍由原 owner 负责。semaphore 保留 binary/timeline 类型，host timeline wait 拒绝 binary 对象。
- command pool 销毁后以 null handle 表示失效，重复 destroy 无操作，handle/reset/free 拒绝失效 pool。
  批量 free 先验证 command 的 pool/device 来源及同批唯一性；owner 仍负责 GPU 完成及撤销所有 alias。
  command wrapper 的 Clone 用于 FIF 集中回收和跨帧复用，不延长 native command/pool/device 生命周期。
- begin 先校验标签再进入录制状态。`GfxImmediateCtx::one_time_exec` 顺序执行 分配 → begin → 录制 →
  end → submit → wait_idle → free；闭包只录制，不自行提交/free 或让 command alias 逃逸。
  返回值和闭包持有资源的显式销毁仍归各自 owner。
- 同步 buffer 上传（`transfer_data_sync` / `transfer_data_sync2`）共用一条 staging 路径：检查容量，
  空上传不调用 callback、不分配、不提交；callback 填写 staging 后统一 flush，one-time 等待完成后销毁 staging。
  仅填写 staging 的回调不得自行提交或让映射指针逃逸。
- image/index/SoA、structured SSBO 的带数据构造与同步 AS build/compact 都基于上述同步路径，
  返回前 GPU 工作已完成，临时资源随即显式销毁。
- graphics pipeline 构造结束时显式销毁临时 shader cache；销毁 pipeline 时仅在没有外部 Rc owner 时释放 layout。
- SBT 大小和偏移使用 checked DeviceSize 算术，分配前检查 host 范围、group 索引、设备对齐与 handle stride
  上限；分配后检查非零 base address、对齐和完整地址范围。raygen 的 size 等于 stride，空区域保持零。
  这些检查不证明 group 类型与 pipeline 一致，也不改变已提交 GPU work 的销毁前置。
- SoA 的属性偏移、固定大小和额外 UV tail 均检查溢出及 host 范围；mesh 的 device/staging 分配复用
  `VertexLayoutSoA3D::extra_uv_bytes`。合法布局保持不变，固定布局始终保留一个 UV 槽。
- `GfxQueryPool` 只承载单个 `u64` 标量结果（occlusion、timestamp 和四种 AS property query），
  读取固定使用 `TYPE_64 | WAIT`；多计数器或可变布局查询需要另行表达其数据契约。
  read/reset/AS property write 在 native 调用前验证容量、溢出及适用类型，合法空范围不调用 native。
  类型检查不证明设备 feature 已启用；owner 仍负责 query reset/reuse 顺序与 GPU 完成条件。
- VMA-backed buffer/image 创建使用 `resources::vma_debug::with_vma_debug_name` 写入 copied allocation user data。调用点不应直接设置裸 `user_data` 指针。
- buffer/image/view/semaphore 的 `destroy` 先记录 trace 日志，再释放 native 对象；buffer 的 device address
  和容量随 destroy 失效。special buffer 复用底层 buffer 的 debug naming。
- `GfxAccelerationScratchBuffer` 按设备的 `minAccelerationStructureScratchOffsetAlignment` 分配；BLAS/TLAS 不能依赖普通 buffer 默认对齐或 VMA 分配顺序碰巧满足地址要求。该只读对齐值由 `GfxResourceCtx` 传入，scratch 的释放时机仍由 build owner 决定。
- `new_blas_for_build` 只分配 AS storage 与 scratch，不提交 GPU 工作；同步 BLAS build 复用它完成尺寸查询与分配。
  BLAS geometry 数量在投影前检查 u32 和设备上限，并限制所有 geometry 的 primitive 总数；TLAS 在上传前
  检查非空、u32 和设备 instance 上限。限制借用 root 已查询的物理设备属性；空场景由上层撤销 TLAS。
- 连续 TLAS instance buffer 显式请求 16 字节对齐；AS command 在进入 native 前核对 ranges 数量与
  geometryCount。裸 geometry 地址的分配来源、有效内容与 GPU 可见性仍由输入 owner 保证。
- TLAS 在 build 前显式建立 transfer-write 到 AS-build 输入读取的屏障；同一队列上的 host wait 只证明完成，
  不保证 transfer 写入对 build 输入可见。同步 BLAS/TLAS helper 在 build/compact 等待完成后显式销毁临时资源。

## 设计意图

- typed Ctx 把 Vulkan 能力按阶段拆窄：resource allocation、descriptor/pipeline、queue submit、surface/swapchain、device info 和 immediate helper 分别使用对应 context。
- `GfxDevice` 统一启用 bindless descriptor indexing 与 sampled / storage image 的 non-uniform array indexing；
  当前全局 bindless 只保留 `bindless_srvs[]`，shader 通过 `NonUniformResourceIndex` 按 Material/Scene
  数据动态访问 asset texture/sky 时依赖该 logical device feature 契约。pass-local storage/sampled descriptor
  不依赖全局 slot，但仍复用同一套 Vulkan descriptor 基础能力。
- 长期资源对象不保存 `&Gfx`、`&GfxDevice` 或 allocator 引用，销毁依赖由 owner 在调用点传入。
- `Drop` 只做遗漏销毁诊断，避免在不可控 drop 顺序中调用 Vulkan/VMA/WSI release API。

## 依赖关系

- 上层几乎所有渲染模块都依赖本 crate
- 本 crate 不应依赖更高层业务语义模块

映射与初始化分别由 `GfxBuffer` 构造入口控制：readback 与提供 safe typed slice 的 structured mapped buffer 清零，
raw staging/SBT 使用只写映射，不做全量清零。调用方必须写满实际上传/消费区域；SBT 只消费已写入的 group handle，
不读取 stride padding。只写映射禁止 `transfer` 的 safe POD 原地借用；raw pointer 的初始化与访问责任仍在调用方。
flush/invalidate 与 GPU 使用完成前置保持不变。
