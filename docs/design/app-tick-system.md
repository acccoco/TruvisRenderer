# App 侧 Tick 机制

> 日期：2026-04-17
> 状态：设计草案（未实现）


## 一、动机

当前 `truvis-app` 中两处耦合不合理：

1. **`CameraController` 硬编码在 `RenderApp` 里**。`RenderApp` 的定位是"编排层"，相机控制器是一种"app 级别的行为"，不应在编排层固定出现。不同 demo 可能需要不同的相机交互方式（FPS、轨道、固定机位），硬编码意味着只能用一种。
2. **`OuterApp::update()` 只收到 `&mut Renderer`**，拿不到 `InputState`、`dt`、`viewport`。想在 outer app 自己做输入驱动的逻辑（例如 `shader-toy` 里切换模式）只能绕道。

本文档记录一个最小的 tick 机制设计，用于：

- 把 `CameraController` 从 `RenderApp` 的固定字段中解耦，让具体 outer app 自行注册
- 为未来其他"每帧行为"（物理、动画、自定义控制器）提供统一接入点


## 二、既有流程还原

```
WinitApp::window_event(RedrawRequested)
        │
        ▼
RenderApp::big_update()
 ├─ begin_frame
 ├─ 处理 winit 事件 → InputManager
 ├─ 可能 recreate_swapchain → outer_app.on_window_resized()
 ├─ acquire_image
 ├─ build_ui → outer_app.draw_ui(ui)
 ├─ update_scene(input_state) {
 │     camera_controller.update(input_state, viewport, dt)   ← 硬编码
 │     outer_app.update(&mut renderer)                        ← 参数贫乏
 │  }
 ├─ renderer.before_render(camera_controller.camera())        ← 需外部喂 camera
 ├─ outer_app.draw(...)
 ├─ present_image
 └─ end_frame
```

两点摩擦：

- `CameraController` 拥有 `Camera`，而 `Camera` 类型定义在 `truvis-renderer`，所有权链绕了一圈
- 编排层 `RenderApp` 与具体行为 `CameraController` 没有通过抽象隔离


## 三、讨论过的设计维度

### 3.1 谁调 tick

| 路径 | 描述 |
| --- | --- |
| A. engine 提供注册表 | `RenderApp` 维护 `Vec<Box<dyn Tickable>>`，outer app 通过注册句柄加入 |
| B. outer app 自管 | engine 不知情，只把 context 丰富后传给 `OuterApp::update()` |

**决策：A**。理由：`RenderApp` 是编排层，由它统一驱动 tick 与"编排"定位一致；同时 outer app 只需在 `init` 里注册，后续不用操心遍历。

### 3.2 Camera 所有权

| 方案 | Camera 归属 | CameraController 归属 |
| --- | --- | --- |
| 1 | renderer | app（维持现状：controller 持有 camera） |
| 2 | app | app（engine 只认 POD 快照） |
| 3 | renderer | app（controller 仅是行为，不持有 camera） |

**决策：方案 3**。Camera 作为 engine 公开的可变状态槽，由 `Renderer` 持有；`CameraController` 变为纯行为，每帧通过 `TickContext` 拿 `&mut Camera` 进行修改。

> [!info] 为何不选方案 2
> 方案 2 最"干净"，但需要重构 `Renderer::before_render` 及相关 GPU 上传路径，改动面大。方案 3 既让 controller 脱离 engine，又把 Camera 的"初值配置 / 每帧修改 / GPU 上传"三件事的边界清晰化，改动可控。

### 3.3 Tick 阶段

**决策：单阶段**。所有 tickable 按注册顺序依次 tick。多阶段（pre/post/fixed）是过早优化，当前只有一个 camera controller 的场景不需要。

### 3.4 Resize 事件

**决策：tickable 不关心 resize**。`OuterApp::on_window_resized` 保持现状，由 outer app 自己处理。tickable 若需知道 viewport 变化，可在 `tick()` 中读取 `ctx.viewport`。


## 四、最终设计

### 4.1 核心类型

定义于 `truvis-app::platform::tickable`：

```rust
pub trait Tickable {
    fn tick(&mut self, ctx: &mut TickContext);
}

pub struct TickContext<'a> {
    pub dt: std::time::Duration,
    pub frame_id: u64,
    pub input: &'a InputState,
    pub viewport: vk::Extent2D,
    pub camera: &'a mut Camera,
}
```

**设计约定**：

- `TickContext` 故意不暴露 `&mut Renderer`。避免 tick 与 `OuterApp::update` 职责混淆。未来若 tickable 需要访问 renderer 的其他子系统，在 context 上显式新增字段。
- `frame_id` 当前无使用方，但字段成本极低，预留。

### 4.2 注册接口

```rust
// OuterApp::init 签名变更
trait OuterApp {
    fn init(
        &mut self,
        renderer: &mut Renderer,
        registry: &mut TickRegistry,     // 新增
    );
    // ...
}

// TickRegistry 是 RenderApp 内部 Vec 的受限访问接口
pub struct TickRegistry<'a> { /* 封装 &mut Vec<Box<dyn Tickable>> */ }
impl<'a> TickRegistry<'a> {
    pub fn register(&mut self, t: Box<dyn Tickable>);
}
```

注册顺序即 tick 顺序。无注销接口（无用例）。

### 4.3 Renderer 改动

```rust
pub struct Renderer {
    pub camera: Camera,     // 新增字段
    // ...
}

impl Renderer {
    // 去掉 camera 参数，内部使用 self.camera
    pub fn before_render(&mut self);
}
```

`Camera` 的初值由 outer app 在 `init` 里通过 `&mut Renderer` 直接配置。

### 4.4 CameraController 改动

```rust
// 去掉 camera 字段
pub struct CameraController { /* 仅配置，例如移动速度、鼠标灵敏度 */ }

impl Tickable for CameraController {
    fn tick(&mut self, ctx: &mut TickContext) {
        // 操作 ctx.camera，读取 ctx.input / ctx.dt / ctx.viewport
    }
}
```

### 4.5 每帧调用顺序

```
update_scene() {
    let input = self.input_manager.state().clone();
    let viewport = self.renderer.render_context.frame_settings.frame_extent;
    let dt = self.renderer.timer.delta_time();
    let frame_id = self.renderer.render_context.frame_counter.frame_id();

    let mut ctx = TickContext {
        dt, frame_id, viewport,
        input: &input,
        camera: &mut self.renderer.camera,
    };
    for t in &mut self.tickables {
        t.tick(&mut ctx);
    }

    self.outer_app.as_mut().unwrap().update(&mut self.renderer);
}

self.renderer.before_render();   // 不再传 camera
```

借用分析：tick 阶段仅借用 `self.renderer.camera` 字段，与之后全量借用 `&mut self.renderer` 分处两个作用域，无冲突。


## 五、影响面

| 文件 | 改动 |
| --- | --- |
| `truvis-renderer/src/renderer.rs` | 添加 `camera` 字段；`before_render` 去掉参数 |
| `truvis-app/src/platform/tickable.rs` | **新增** `Tickable`、`TickContext`、`TickRegistry` |
| `truvis-app/src/platform/camera_controller.rs` | 去掉 `camera` 字段；实现 `Tickable` |
| `truvis-app/src/render_app.rs` | 删除 `camera_controller` 字段；添加 `tickables: Vec<Box<dyn Tickable>>`；`init_after_window` 构造 `TickRegistry` 传给 `OuterApp::init`；`update_scene` 改为遍历 tickables |
| `truvis-app/src/outer_app/base.rs` | `OuterApp::init` 签名加 `registry: &mut TickRegistry` 参数，去掉 `camera: &mut Camera` 参数 |
| `truvis-app/src/outer_app/{triangle, shader_toy, cornell_app, sponza_app}/*` | 各 demo 的 `init` 里：通过 `renderer.camera` 配置相机初值；`registry.register(Box::new(CameraController::new()))` |


## 六、开放问题

以下问题在当前设计中留作后续演进，不阻塞落地：

- **Tickable 的依赖与排序**：若将来出现多个 tickable 之间有依赖关系（例如"动画 tick 必须在物理 tick 之后"），当前的"注册顺序即执行顺序"是否足够？可能需引入阶段或优先级。
- **Tickable 访问更多 engine 子系统**：物理、动画等 tickable 可能需要访问 `AssetHub`、`SceneManager`。届时在 `TickContext` 上显式添加字段，避免口子开在 `&mut Renderer` 上。
- **Tickable 的生命周期事件**：当前只有 `tick`。未来是否需要 `on_register` / `on_unregister` / `on_resize`？暂不引入。
- **Camera 的最终归属**：如果 engine 要进一步精简（对应"理想模块架构"中 engine 仅认 GPU 数据的方向），Camera 迁出 engine、改为方案 2 仍是后续选项。


## 七、决策快照

| 项 | 决策 |
| --- | --- |
| Tick 调度者 | `RenderApp` |
| 注册方 | outer app 在 `OuterApp::init` 中通过 `TickRegistry::register` |
| Camera 归属 | `Renderer`（方案 3） |
| Tick 阶段 | 单阶段 |
| Tick 顺序 | 注册顺序（FIFO） |
| Resize 处理 | tickable 不介入，维持 `OuterApp::on_window_resized` |
| `TickContext` 是否含 `&mut Renderer` | **否** |
| 注销接口 | 不提供 |
