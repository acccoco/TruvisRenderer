# ARCHITECTURE.md

本文是项目当前架构入口和导航页，只保留最高优先级约束与详细文档入口。完整分层、生命周期、数据流和资源契约请阅读
`docs/summaries/` 下的主题文档；具体 crate 入口、文件职责和运行命令请查看对应模块 README。

## 推荐阅读顺序

1. [`docs/summaries/layering-and-dependency-boundaries.md`](docs/summaries/layering-and-dependency-boundaries.md)
   ：总体分层、依赖方向、app / engine 边界。
2. [`docs/summaries/frame-lifecycle.md`](docs/summaries/frame-lifecycle.md)：启动、render loop、`RenderRuntime` / App /
   Plugin phase 顺序。
3. [`docs/summaries/runtime-app-plugin-boundaries.md`](docs/summaries/runtime-app-plugin-boundaries.md)：状态所有权、Ctx
   裁剪、`RenderAppShell` 与 Plugin 模型。
4. [`docs/summaries/render-graph-and-data-flow.md`](docs/summaries/render-graph-and-data-flow.md)：AssetHub 到 GPU scene
   的同步路径、RenderGraph pass 编排规则。
5. [`docs/summaries/threading-and-resource-lifecycle.md`](docs/summaries/threading-and-resource-lifecycle.md)：主线程 /
   渲染线程边界、GPU 同步、资源创建 / 重建 / 销毁契约。

## 全局架构约束

- 项目保持无环依赖：上层可以依赖下层，下层不反向依赖上层业务；同层 crate 默认不互相依赖，除非 summaries 中明确记录。
- 平台层只负责窗口、事件循环和渲染线程启动；渲染线程通过 `Box<dyn RenderApp>` 驱动 App，并创建、使用、销毁所有 Vulkan 对象。
- `RenderRuntime` 拥有 `Gfx`、`World`、`GpuStore`、`GpuScene`、present、cmd 和同步资源；App / Plugin 只通过 phase-appropriate
  Ctx 使用窄能力，不长期持有完整 runtime 或 typed `Gfx` Ctx。
- App state 持有 GUI、camera/input、overlay 和具体渲染管线能力，并显式决定 RenderGraph pass 顺序；标准 `Plugin` trait
  只承载通用生命周期，特有能力由具体类型方法暴露。
- GPU 资源以显式 owner 为生命周期边界；Vulkan/VMA/WSI wrapper 通过显式 `destroy` 路径释放，`Drop` 不调用底层 release API。

## 文档职责

- `ARCHITECTURE.md`：当前架构入口、阅读顺序和最高优先级约束。
- `docs/summaries/`：当前实现事实总结，记录分层、生命周期、状态所有权、数据流、线程和资源契约。
- 模块内 `README.md`：说明模块职责、依赖与常见入口。
- `docs/brain-storm/`：记录设计讨论、方案评估和开放方向；归档内容不作为当前事实来源。
