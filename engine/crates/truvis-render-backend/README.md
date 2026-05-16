# truvis-render-backend

渲染后端整合层，负责持有 `World` / `RenderWorld` 并通过生命周期方法暴露 typed Ctx。

## 主要职责

- 持有 `Gfx` root owner，并作为 typed Gfx Ctx 的生命周期来源
- 提供 `begin_frame`、`update_phase`、`prepare`、`render_phase`、`present`、`end_frame`
- 产出 `RenderBackendInitCtx` / `RenderBackendUpdateCtx` / `RenderBackendRenderCtx` / `RenderBackendResizeCtx` / `RenderBackendShutdownCtx`
- 与 swapchain / command 提交 / 同步机制对接

## 状态所有权

- `RenderBackend` 持有 `Gfx` root owner，并负责在所有子资源之后销毁它。
- `World` 承载 CPU 侧 scene/assets，供 update / prepare 阶段读取或修改。
- `RenderWorld` 承载 GPU 侧 frame state、global descriptors、bindless、manager-owned resources、FIF buffers 和 frame settings。
- `AssetTextureUploader` 消费 `AssetHub` 的 texture CPU bytes，负责 GPU image / view / bindless 注册。
- `AssetMeshUploader` 消费 `AssetHub` 的 mesh CPU 数据，负责 vertex/index buffer 上传、BLAS build 和 mesh GPU ready 查询；copy 后到 BLAS build 前必须同步 `TRANSFER_WRITE -> ACCELERATION_STRUCTURE_BUILD_KHR`，并覆盖 device address 输入的 `SHADER_READ` 访问。
- `MaterialBridge` 由 backend 持有，负责把 `AssetHub` 的 CPU material 同步为稳定 GPU material slot；backend 私有 `MaterialManager` 持有 material buffer、dirty 上传和延迟 slot 回收。
- `InstanceBridge` 由 backend 持有，负责 `InstanceHandle -> GpuInstanceSlot` 稳定映射、ready gate 和 active render list。
- Assimp scene 文件读取不在 backend 内执行；backend 只消费 `AssetHub` 产出的 texture / mesh CPU 事件，并忽略或记录 scene ready / failed 事件。
- backend 不再保留 `model_loader::AssimpSceneLoader` 兼容 facade；scene 导入入口统一为 `AssetHub::load_scene()` 和 `SceneManager::spawn_scene_asset()`。
- `RenderPresent` 管理 surface、swapchain、present image 和窗口尺寸相关资源。

## 生命周期边界

- `RenderBackend::new` 创建 `Gfx` 并通过 typed Ctx 初始化 backend-owned GPU 资源。
- `RenderBackend::init_after_window` 创建 surface、swapchain 与 `RenderPresent`，并把 init 阶段所需的 typed Ctx 交给 app/plugin。
- `update_phase` 产出可修改 `World` 和相关帧设置的 `RenderBackendUpdateCtx`。
- `prepare(camera)` 在 update 与 render 之间同步 CPU 语义数据到 GPU 可见资源。
- `render_phase`、`handle_resize`、`shutdown_phase` 只暴露当前阶段需要的 device/resource/queue/surface/immediate/device-info Ctx。
- `wait_idle` 由 runtime 在 app/plugin shutdown 前调用，确保 plugin-owned pipeline、buffer、descriptor 等资源销毁前 GPU 不再引用上一帧 command buffer。
- `destroy` 先等待 GPU idle，再释放 present、FIF、assets、GPU scene、mesh uploader、command allocator、sync、descriptor 等子资源，最后销毁 `Gfx` root owner。

## Tracy 初始化埋点

- `RenderBackend::new` 使用一级 span 标记 backend 启动阶段的主要初始化步骤，例如 `Gfx`、manager、asset texture uploader、material bridge、GPU scene、FIF buffers、global descriptors、sampler、per-frame buffer 和 command buffer 创建。
- 启动耗时较明显的下层构造函数继续使用二级 span 细分，例如 `AssetTextureUploader::new`、`GpuScene::new`、`FifBuffers::new`、`GlobalDescriptorSets::new`、`CmdAllocator::new` 和 `RenderSamplerManager::new`。
- `SceneManager::new` 不在 `truvis-scene` 内部添加 Tracy 依赖；它只通过 `RenderBackend::new/scene_manager` 这个一级 span 表示。

## 设计意图

- backend 不依赖 `RenderApp`、`RenderAppHooks`、`Plugin` 或具体 demo，因此可以作为 App shell 之下的纯渲染执行层。
- 上层只能通过 lifecycle methods 和 `RenderBackend*Ctx` 访问阶段能力，避免长期保存完整 `&Gfx` 或直接借用 backend 内部字段。
- `prepare` 接收 App 提供的 camera，保持 camera/input state 归属 App，而不是放入 backend。
- GUI draw data 不进入 backend 通用 Ctx；GUI 的 RenderGraph 集成由 `truvis-app::gui_plugin::GuiPlugin` 完成。

## 与其他模块关系

- 上承 `truvis-frame-runtime::RenderAppShell`（帧骨架）与 `truvis-app`（插件编排）
- 下接 `truvis-gfx`、`truvis-render-interface`、`truvis-render-graph`
- 不依赖 `RenderApp`、`Plugin`、GUI plugin 或具体 demo app
