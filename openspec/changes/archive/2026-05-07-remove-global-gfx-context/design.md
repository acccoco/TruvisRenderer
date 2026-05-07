## Context

当前 `Gfx` 通过 `static mut G_GFX` 作为全局单例存在，`Gfx::get()` 被 `truvis-gfx` 底层 wrapper、RenderBackend、RenderPresent、RenderGraph pass、asset/gui/app 代码广泛调用。上一轮 `World` / `RenderWorld` 拆分已经让上层状态通过生命周期 Ctx 显式传递，但 `truvis-gfx` 层仍把 Vulkan device、allocator、queue、instance、physical device 等依赖隐藏在全局访问中。

资源生命周期目前是混合模型：`GfxImage` / `GfxImageView` / `GfxSemaphore` / `CmdAllocator` 等已经偏向显式销毁和 `Drop` 诊断；`GfxBuffer`、descriptor layout/pool、pipeline、sampler、query 等仍在 `Drop` 中释放 Vulkan/VMA 资源；`GfxSurface` 仍在 `Drop` 中释放 WSI surface。为了彻底移除全局上下文，销毁路径也需要显式接收上下文。

除创建/销毁外，若干工具能力也隐藏在 `Gfx::get()` 后面，例如 `one_time_exec`、`wait_idel`、debug name/label、queue family、`min_ubo_offset_align`、`rt_pipeline_props`、format support 查询等。这些能力同样需要通过 typed Gfx Ctx 表达，否则全局单例会以 helper 形式残留。

## Goals / Non-Goals

**Goals:**

- 移除 `G_GFX` 全局对象和 `Gfx::get()` 单例访问。
- 让 `RenderBackend` 成为 `Gfx` root owner，并通过生命周期 Ctx 向下传递 typed Gfx Ctx。
- 统一 Vulkan/VMA/WSI wrapper 生命周期：创建、使用、销毁所需上下文必须通过参数或 owner 字段显式可见。
- 将 `Drop` 改为 debug 诊断入口，不在 `Drop` 中调用 Vulkan/VMA/WSI release API。
- 将 one-time command、device wait idle、debug naming/labels、queue family、device limits/properties、format support 查询等 helper 能力纳入 typed Ctx 迁移范围。
- 保持渲染行为、帧顺序、线程归属、RenderGraph 同步和资源销毁顺序不变。
- 更新架构文档，明确显式 owner-owned 生命周期规则。

**Non-Goals:**

- 不引入多 GPU、多 device 或跨线程共享 Vulkan 对象。
- 不改变 RenderGraph 资源依赖推导或 pass 执行语义。
- 不把 `Gfx` 暴露为 plugin 可任意使用的大型全能上下文。
- 不把 typed Gfx Ctx 存入 `RenderWorld` 内部资源对象以传播长期 lifetime。
- 不顺手重构 shader、FFI、scene/asset 数据模型。

## Decisions

### D1: `RenderBackend` 持有 `Gfx` root owner

**选择**：`RenderBackend` 直接拥有 `Gfx`，`RenderBackend::new` 通过 `Gfx::new(...)` 构造 root owner，`RenderBackend::destroy` 在所有 GPU 资源显式释放后销毁 `Gfx`。

**理由**：

- `RenderBackend` 已经是渲染线程中 World、RenderWorld、present/cmd/sync 生命周期的聚合 owner。
- 将 `Gfx` 放在 `RenderBackend` 内，可以让 root Vulkan 生命周期与现有 RenderBackend shutdown 顺序对齐。
- 渲染线程仍只通过 `Box<dyn RenderApp>` 驱动 app；外部不需要知道 `Gfx` 细节。

**备选方案**：由 RenderAppShell 持有 `Gfx` 并传给 RenderBackend。拒绝原因是 RenderBackend 才是所有底层 Vulkan 资源的直接生命周期边界，Shell 持有会扩大 runtime 层职责。

### D2: 使用窄 typed Gfx Ctx，而不是单一大 `GfxCtx`

**选择**：`Gfx` 提供若干借用视图，例如 `GfxDeviceCtx`、`GfxResourceCtx`、`GfxQueueCtx`、`GfxSurfaceCtx`、`GfxDeviceInfoCtx`、`GfxImmediateCtx`。调用点按实际能力选择最窄 Ctx。

**理由**：

- 降低散传 `device` / `allocator` / `queue` 的语法噪声。
- 保留静态可分析性：函数签名仍能看出需要 device、allocator、queue、surface、device properties/limits 或 immediate command 相关上下文。
- 避免一个全能 `GfxCtx` 退化为“参数形式的全局对象”。

**备选方案**：所有 API 统一接收 `&Gfx`。拒绝原因是这会让依赖过宽，上层无法从签名看出真实能力需求。

### D3: typed Gfx Ctx 只作为临时借用视图，不存进资源对象

**选择**：GPU 资源对象保存 Vulkan handle、allocation、debug name、destroyed/null 状态；不保存 `GfxCtx`、`&GfxDevice` 或 `&VMemAllocator`。

**理由**：

- 如果资源对象保存借用 Ctx，`RenderWorld`、GfxResourceManager、Plugin 字段会被 lifetime 污染，整体复杂度显著上升。
- 显式销毁模型的核心是 owner 在生命周期阶段传入上下文，而不是资源自己长期携带上下文。
- 对象字段保持接近 Vulkan handle wrapper，有利于 resize、manager 延迟销毁和数组管理。

**备选方案**：资源对象保存 `Rc<GfxDevice>` / `Rc<VMemAllocator>`。拒绝原因是它会隐藏销毁依赖，且与“尽量显式销毁”的目标相冲突。

### D4: `Drop` 不释放 Vulkan/VMA/WSI，只做 debug 断言

**选择**：所有 Vulkan/VMA/WSI wrapper 的 `Drop` 不调用 Vulkan/VMA/WSI release API。若对象持有有效 handle 或 allocation 且未显式销毁，debug 构建 SHALL 触发断言。

**理由**：

- `Drop` 无法接收 typed Gfx Ctx；继续在 Drop 中释放资源会迫使全局访问或隐藏引用。
- 显式 destroy 让销毁顺序由 owner 代码表达，尤其适合 Vulkan 的依赖顺序。
- 现有多个 manager-owned 类型已经采用 `Drop` 诊断模式，迁移方向一致。
- `GfxSurface` 这类 WSI 对象也必须纳入该规则，否则 `Window` / `VkSurfaceKHR` 的二阶段关闭顺序仍隐藏在 Drop 中。

**备选方案**：继续保留 RAII Drop，并让资源存 `Rc` 上下文。拒绝原因是静态依赖不够显式，且 root owner 销毁顺序更难审查。

### D5: 同时提供 `destroy(self, ctx, reason)` 与必要的 `destroy_mut`

**选择**：叶子资源优先提供消费自身的 `destroy(self, ctx, reason)`；需要 resize、manager 延迟释放、数组/字段原地复用的类型提供 `destroy_mut(&mut self, ctx, reason)` 并置空 handle。surface/swapchain 这类 WSI wrapper 也遵循同一规则。

**理由**：

- `destroy(self, ...)` 可由 Rust move 语义阻止销毁后继续使用。
- `destroy_mut` 对 manager、swapchain resize、FIF buffers、RenderPresent 这类组合 owner 更实用。
- 两者都必须使 `Drop` 看到“已销毁”状态。

**约束**：同一类型如果同时提供两种 API，`destroy(self, ...)` SHALL 只是调用 `destroy_mut(...)` 后消费自身，不得有两套释放逻辑。

### D6: 迁移从 `truvis-gfx` 叶子类型开始，再向上层收敛

**选择**：先在 `Gfx` 中引入 root owner 构造和 typed Ctx；再改 `truvis-gfx` 资源、同步、descriptor、pipeline、swapchain/surface；最后迁移 render-interface、render-backend、render-passes、asset/gui/app 调用点并删除全局 API。

**理由**：

- 底层类型决定上层签名，先改上层会反复返工。
- 可以在过渡期暂时保留 `Gfx::get()` 作为未迁移调用点的兼容入口，但最终任务必须删除。
- 每个阶段都可以通过 `rg "Gfx::get"` 和 cargo check 控制剩余面。

## Risks / Trade-offs

**[Risk] 忘记显式 destroy 导致 release 构建泄漏** -> `Drop` debug_assert、集中 shutdown 测试、`rg "impl Drop"` 审查，以及在 manager/backend destroy 中覆盖所有 owner 字段。

**[Risk] `destroy_mut` 让已销毁对象仍可被误用** -> 销毁后置 null/destroyed flag；公开方法对有效 handle 做 debug/assert 检查；叶子类型优先使用 `destroy(self, ...)`。

**[Risk] typed Ctx 过大导致隐式耦合换皮** -> 每类 API 只接收最窄 Ctx；spec 和 review 中禁止把完整 `&Gfx` 传给无需完整能力的函数。

**[Risk] 隐式 helper 访问残留** -> 除资源构造/销毁外，单独审查 `one_time_exec`、`wait_idel`、debug name/label、queue family、device limits/properties、format support 查询等 helper，避免它们继续通过 `Gfx::get()` 隐式取依赖。

**[Risk] WSI surface 仍由 Drop 销毁** -> `GfxSurface` / `GfxSwapchain` 必须提供显式 destroy，并由 `RenderPresent::destroy` 在 `Gfx` root owner 和 winit `Window` drop 前释放。

**[Risk] 迁移面大，容易漏掉全局访问** -> 分阶段迁移并用 `rg "Gfx::get\\(|Gfx::init\\(|Gfx::destroy\\(|G_GFX"` 作为完成门槛。

**[Risk] shutdown 顺序回归** -> 任务中单独验证 RenderAppShell shutdown、RenderBackend::destroy、RenderPresent resize/destroy、GfxResourceManager destroy、Plugin shutdown 路径。

**[Trade-off] API 参数增多**：显式 Ctx 会增加部分函数签名长度，但换来依赖可见和更清晰的生命周期边界。

## Migration Plan

1. 新增 `Gfx::new(...)`、`Gfx::destroy(self)` 和 typed Gfx Ctx 借用视图，覆盖 device/resource/queue/surface/device-info/immediate command 等能力，暂时保留旧全局 API。
2. 将 `RenderBackend` 改为持有 `Gfx`，并在内部构造 `World` / `RenderWorld` / `RenderPresent` 时传递 typed Ctx。
3. 迁移 `truvis-gfx` 叶子类型和 helper：buffer/image/image_view、sync、command pool/buffer、descriptor、pipeline、shader、sampler、query、acceleration、swapchain/surface、one-time exec、debug naming/labels、device properties/limits 查询。
4. 迁移 `truvis-render-interface` 组合 owner 和 manager：CmdAllocator、GfxResourceManager、GlobalDescriptorSets、FifBuffers、GpuScene、BindlessManager、SamplerManager。
5. 迁移 `truvis-render-backend`、render-passes、render-graph、asset、gui、app 调用点。
6. 删除 `G_GFX` 与 `Gfx::get/init/destroy`，更新文档，运行格式化和仓库 check。

## Open Questions

- typed Ctx 是否按 `Copy` 值传递还是按 `&Ctx` 传递：当前建议 Ctx 内部只含引用且 `Copy`，函数接收 by-value，调用更轻量。
- `DestroyReason` 是否应上移到 `truvis-gfx` 通用生命周期模块：当前它位于资源 lifecycle 语境，若 descriptor/pipeline/sampler 也需要 reason，可能需要扩展命名或分层。
- `GfxDeviceInfoCtx` 是否独立于 `GfxDeviceCtx`：当前建议独立表达 physical-device properties、limits、format support 和 ray tracing properties，避免只读查询 API 被迫接收完整 device 操作能力。
- release 构建是否需要轻量泄漏日志：debug assert 已足够暴露开发期遗漏；release 日志可能引入噪声，暂不默认要求。
