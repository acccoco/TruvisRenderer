# ARCHITECTURE.md

本文档描述项目的总体结构、核心设计思路与模块边界约束。
实现细节可继续查阅各模块目录下的 `README.md`。

## 1. 总体结构

```text
Render-Rust-vk-Truvis/
├─ engine/
│  ├─ crates/        # Rust 引擎核心模块（分层）
│  ├─ shader/        # Slang shader 源码、共享头与构建工具
│  └─ cxx/           # C++ 资产加载与 FFI 绑定
├─ truvis-winit-app/ # 窗口与事件循环入口（可执行程序）
├─ tools/            # 构建辅助与路径工具
├─ assets/ resources/# 资产与运行资源
└─ docs/             # 设计文档与效果截图
```

## 2. 分层设计

项目以无环依赖为目标，按职责划分为以下层次：

```text
L0 Foundation
  truvis-utils / truvis-logs / truvis-path / descriptor-layout / shader-binding

L1 RHI
  truvis-gfx

L2 Render Contract
  truvis-render-interface

L3 Domain + Graph (同层互不依赖)
  truvis-render-graph / truvis-scene / truvis-asset / truvis-gui-backend / truvis-world

L4 Renderer Integration
  truvis-renderer

L5 App Contract + Runtime
  truvis-app-api (plugin contract + typed contexts + overlay)
  truvis-frame-runtime (phase orchestration runtime)
  truvis-render-passes (shared pass implementations, depends on render-interface, NOT renderer)
  truvis-app (demo apps + RenderGraph integration)

L6 Platform Entry
  truvis-winit-app
```

主干依赖链：

```text
truvis-gfx
  -> truvis-render-interface (defines RenderWorld)
      -> truvis-render-graph
      -> truvis-world (defines World, depends on scene + asset)
          -> truvis-renderer (holds World + RenderWorld)
              -> truvis-app-api (contexts use World + RenderWorld)
                  -> truvis-frame-runtime
                      -> truvis-app (demos + integration)
                          -> truvis-winit-app
      -> truvis-render-passes (shared passes, depends on render-interface, NOT renderer)
```

## 3. 核心模块职责

- `truvis-gfx`：Vulkan RHI 抽象，封装设备、队列、资源、同步与管线对象。
- `truvis-render-interface`：渲染契约层，提供 FrameCounter、CmdAllocator、Handle、全局描述符、资源管理基础设施与 `RenderWorld`（GPU 渲染状态聚合容器）。
- `truvis-render-graph`：声明式 pass 编排，负责资源状态推导与同步拼接。
- `truvis-scene`：CPU 侧场景数据组织（mesh/material/instance/light）。
- `truvis-asset`：异步资产加载与上传流程。
- `truvis-world`：CPU 侧场景状态聚合（`World`），持有 `SceneManager` + `AssetHub`，与 GPU 状态物理分离。
- `truvis-renderer`：backend 执行与子系统整合，持有 `World`（CPU 状态）+ `RenderWorld`（GPU 状态）。
- `truvis-app-api`：`AppPlugin` 插件契约、typed contexts（`InitCtx`→World+RenderWorld / `UpdateCtx`→World / `RenderCtx`→&RenderWorld / `ResizeCtx`→&mut RenderWorld）与 overlay 合约。
- `truvis-frame-runtime`：`FrameRuntime` 帧编排运行时，外部仅通过 public API 驱动（`push_input_event` / `time_to_render` / `run_frame` / `destroy`）。
- `truvis-render-passes`：通用 render pass 实现（RT / 累积 / 降噪 / 色调映射 / blit / resolve / phong）。
- `truvis-app`：示例应用（triangle / rt-cornell / rt-sponza / shader-toy）、`GuiRgPass` RenderGraph 适配与过渡期 re-export shim。

## 4. 关键数据流

```text
磁盘资产 -> AssetLoader -> AssetHub -> UploadManager -> GPU 资源
CPU Scene -> RenderData -> GpuScene 上传 -> shader 可见 buffer / TLAS
AppPlugin 在 render phase 构建 RenderGraph -> 编译 -> 执行 -> 提交 -> present
```

要点：

- CPU 场景与 GPU 场景是分层建模，通过上传步骤衔接。
- Bindless 索引作为资源访问桥梁，贯穿材质、纹理与渲染 pass。
- RenderGraph 负责声明依赖，减少手写 barrier 与时序错误。

## 5. 运行时序（简化）

```text
begin_frame
  -> 输入处理
  -> acquire swapchain image
  -> UI 构建（runtime overlays + plugin build_ui）
  -> plugin update（CPU 侧更新）
  -> scene/asset 更新与 GPU 上传
  -> 构建并执行 RenderGraph
  -> 提交命令并 present
end_frame
```

补充：

- swapchain 重建由 runtime 单入口触发，覆盖 `size_changed || backend_need_resize`。
- `build_ui` 与 `update` 的顺序按当前实现固定为 `build_ui -> update`。

## 6. 模块边界约束

- `render-graph` 不依赖 `scene`、`asset`，只做图编排。
- `gui-backend` 保持纯 Vulkan 录制能力，RenderGraph 适配放在上层。
- `renderer` 负责整合与调度，不在底层 crate 引入应用语义。
- `app` 层承载应用逻辑与 demo，不向底层反向注入依赖。
- 新增模块时优先保持 DAG 依赖，不引入跨层回边。

## 7. 工程约束

- 渲染示例运行前必须先执行 `cargo run --bin shader-build`。
- 坐标系约定保持一致：Model/View 右手系，NDC 按 Vulkan 约定处理，FrameBuffer 使用 Y 翻转视口。
- C++ 模块通过 CMake + vcpkg manifest 构建，Rust 侧通过 binding crate 调用。

## 8. 延伸阅读

- `engine/README.md`
- `engine/crates/README.md`
- `engine/shader/README.md`
- `engine/cxx/README.md`
