## Why

当前 `truvis-gfx` 通过 `G_GFX` / `Gfx::get()` 隐式获取 Vulkan device、allocator、queue 等上下文，资源创建、使用和销毁依赖无法从函数签名或 owner 边界静态看出。移除全局图形上下文并统一显式销毁，可以降低隐式耦合，让 RenderBackend、RenderWorld、Plugin shutdown 的资源生命周期更可分析。

## What Changes

- **BREAKING**: 移除 `G_GFX` 全局对象，以及 `Gfx::init()` / `Gfx::get()` / `Gfx::destroy()` 单例生命周期 API。
- **BREAKING**: `RenderBackend` 直接持有 `Gfx` root owner，并通过生命周期方法向下传递经过裁剪的 typed Gfx Ctx。
- **BREAKING**: `truvis-gfx` Vulkan/VMA/WSI handle wrapper 不再在 `Drop` 中释放 Vulkan/VMA/WSI 资源；资源释放必须通过显式 `destroy` / `destroy_mut` 完成。
- 新增 typed Gfx Ctx，用于表达不同调用点实际需要的能力，例如 device、resource(device + allocator)、queue、surface/swapchain、device properties/limits、debug naming/labels、immediate one-time command execution 等上下文。
- `Drop` 统一作为 debug 诊断入口：未显式销毁的 Vulkan/VMA/WSI 资源在 debug 构建中触发断言，release 构建不应隐式访问 Vulkan/VMA/WSI。
- 更新 RenderBackend、RenderWorld、RenderPresent、CmdAllocator、GfxResourceManager、Plugin shutdown 等路径，保证 GPU 资源在 `Gfx` root owner 销毁前按明确顺序释放。
- 更新架构文档和 crate README，删除 RAII-owned GPU 资源释放叙述，改为显式 owner-owned 生命周期契约。

## Capabilities

### New Capabilities

- `gfx-explicit-context-lifecycle`: 定义 `truvis-gfx` root owner、typed Gfx Ctx、显式 destroy 规则，以及 Vulkan/VMA/WSI wrapper 的 Drop 诊断契约。

### Modified Capabilities

- `render-threading`: 将渲染线程中的 `Gfx::init` / `Gfx::destroy` 全局生命周期要求更新为 RenderBackend 持有并销毁 `Gfx` root owner。
- `render-backend-lifecycle-ctx`: 扩展 RenderBackend 生命周期 Ctx 契约，使 init、resize、render、shutdown 等阶段可以显式传递所需的 typed Gfx Ctx。

## Impact

- Affected crates: `truvis-gfx`, `truvis-render-interface`, `truvis-render-backend`, `truvis-render-graph`, `truvis-render-passes`, `truvis-asset`, `truvis-gui-backend`, `truvis-frame-api`, `truvis-frame-runtime`, `truvis-app`。
- Affected public API: `Gfx` lifecycle API、GPU resource constructors/destructors、command allocator、descriptor/pipeline/sampler/query/swapchain/surface APIs、RenderBackend/Plugin Ctx fields。
- Affected docs: `ARCHITECTURE.md`, `engine/crates/truvis-gfx/README.md`, `engine/crates/truvis-render-interface/README.md`, `engine/crates/truvis-render-backend/README.md`。
- Expected behavior: no intentional rendering output change; frame order, render thread ownership, resize behavior, RenderGraph synchronization, asset upload behavior should remain equivalent.
