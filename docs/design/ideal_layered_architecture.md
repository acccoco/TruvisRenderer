# 理想的引擎分层架构（Bevy 风格，脱离业务语境）

> 本文是一次架构探索的总结，基于对 D5Engine / d5_lite 现状的分析，参考 Bevy 的设计理念。  
> 从 Rust 的视角审视引擎当前的 subsystem / app / back-pointer 模式，并给出理想的分层结构。

---

## 🎯 本文关注的问题

1. **App 作为上帝对象** —— 所有 subsystem 直接或间接引用 app，所有数据通过 app 访问，但 subsystem 本身归属于 app。这种环形引用是否合理？
2. **层级划分** —— 理想情况下，引擎应该分成哪几层？每层职责是什么？
3. **核心对象归属** —— 场景管理、GPU Scene、Bindless、Asset Loader、Camera Controller、Client Subsystem、Entity 管理这些应该放在哪一层？以什么角色存在？
4. **Main World ↔ Render World 的数据桥** —— Actor 和 GPU Instance Slot 的对应关系如何稳定维护？
5. **Platform ↔ Render World 的交互** —— 窗口、Swapchain、Command 录制由谁发起、谁持有？

---

## 🧠 核心洞见（读完本文该记住的几点）

1. **"Subsystem 的上下文是 World，不是 App"** —— 这是 Bevy / Unreal / Unity 都遵守的纪律。Subsystem 不该持有 App back-pointer。
2. **Resource vs Subsystem 二分** —— 数据（被动）归 Resource，行为（主动、tick）归 Subsystem。当前项目混淆了这两者。
3. **Platform / World / Subsystem 三层寿命域** —— 不同寿命的东西必须分层持有，不能共用一扇访问门（当前的 `app_` 就是共用门）。
4. **Main / Render 双 World** —— 通过 **Extract 单向通道** 协作，两边可以并行。这是 Bevy 最核心的架构选择。
5. **Render World 有自己的持久状态** —— "Render World 只读 Main" 是指每帧数据流向，**不是** Render World 无状态。`Entity ↔ Slot` 映射表就是 Render World 的跨帧 Resource。
6. **窗口是渲染的"输出目标"，不是 Render World 的数据** —— Swapchain 归 Platform，Render World 借用。

---

## 🏛 理想架构总览

```
┌─────────────────────────────────────────────────────────────────┐
│  Layer 0: PLATFORM / HOST                                       │
│  (main.cpp 持有，进程唯一，无 tick，无业务逻辑)                 │
├─────────────────────────────────────────────────────────────────┤
│  CpuExec (JobQueue)   GpuDevice         WindowManager           │
│  AssetServer          BindlessMgr       GpuResourceCache        │
│  Ipc / Net Bus        PlatformEventBus  SurfaceRegistry         │
└────────────────────────┬────────────────────────────────────────┘
                         │ 通过 const ref / Handle<T> 注入下层
                         ▼
┌─────────────────────────────────────────────────────────────────┐
│  Layer -1: APP SHELL  (胶水，不是数据层)                        │
│  • 构造 Platform 资源                                           │
│  • 构造 Main World / Render World 并注入 Platform 引用          │
│  • 驱动 tick: main.tick() → extract() → render.tick()           │
│  • 处理 OS 消息循环                                             │
│  • 不提供 ::get() 单例                                          │
│  • 不持有业务状态                                               │
└────────────────┬──────────────────────┬─────────────────────────┘
                 │                      │
                 ▼                      ▼
┌───────────────────────────┐  ┌──────────────────────────────────┐
│  Layer 1: MAIN WORLD      │  │  Layer 2: RENDER WORLD           │
│  (逻辑世界，CPU-only)     │  │  (GPU 世界)                      │
├───────────────────────────┤  ├──────────────────────────────────┤
│                           │  │                                  │
│  Resources (被动数据):    │  │  Resources (被动数据):           │
│    Time                   │  │    GpuScene                      │
│    InputState             │  │      ├ InstanceBuffer            │
│    SceneGraph = ECS 本身  │  │      ├ LightTable                │
│    AssetHandles<T>        │  │      └ MaterialTable             │
│    Viewports (语义层)     │  │    InstanceSlotAllocator ★       │
│    ClientState            │  │    ExtractedViewports            │
│    GameSettings           │  │    ExtractedMeshes               │
│    WorldEventBus          │  │    ExtractedLights               │
│                           │  │    RenderGraph / FrameContext    │
│  Subsystems (行为/Tick):  │  │    PipelineCache                 │
│    InputSystem            │  │                                  │
│    CameraControlSystem    │  │  Systems (Schedule 按序):        │
│    TransformSystem        │  │    1. Extract*                   │
│    AnimationSystem        │  │    2. Acquire backbuffer         │
│    PhysicsSystem          │  │    3. Prepare GPU buffers        │
│    ClientSystem (IPC)     │  │    4. Queue draw commands        │
│    ScriptingSystem        │  │    5. Record (RenderGraph)       │
│                           │  │    6. Submit                     │
│  ⚠ 不 include GPU 类型   │  │    7. Present                    │
│  ⚠ 无头环境可编译可跑    │  │                                  │
│                           │  │  ⚠ 不反向写 Main World           │
└───────────────────────────┘  └──────────────────────────────────┘
                 ▲                      ▲
                 └────── Extract ───────┘
                       (单向、只读、每帧一次)
```

### 层级访问硬约束

```
✓ 下层不知道上层                                        
✓ 上层持有下层的引用（构造时注入）                      
✗ 下层不 call-back 回上层（Platform 不主动 tick World）
✗ Render World 不写 Main World
✗ Subsystem 不持有 App back-pointer
✗ 没有 ::get() 形式的全局访问
```

---

## 🎭 Bevy 对照表

| Bevy 概念             | 本架构对应          | 说明                                 |
|----------------------|---------------------|--------------------------------------|
| `App`                | AppShell            | 组装者、schedule 驱动                |
| `World`              | Main World          | ECS + Resources                      |
| `SubApp` (RenderApp) | Render World        | 独立 World，独立 schedule            |
| `Res<T>`             | Resource            | 被动数据                             |
| `System`             | Subsystem / System  | 行为，参与 tick                      |
| `Plugin`             | Module              | 一组 Resource + System 注册包        |
| `Handle<T>`          | AssetHandle<T>      | 轻量引用，跨 world 安全              |
| `Assets<T>`          | AssetStorage<T>     | 实际资源存储                         |
| `AssetServer`        | AssetServer         | 加载调度 + handle 生命周期           |
| `Commands`           | DeferredCommands    | 本帧末应用的修改                     |
| `Events<T>`          | EventBus<T>         | 双缓冲事件队列                       |
| Extract schedule     | ExtractPhase        | Main → Render 的唯一数据通道         |
| Render schedule      | Render phases       | Extract/Prepare/Queue/Render         |

---

## 🔑 Resource vs Subsystem 二分（关键概念）

```
┌──────────────────────────────────────────────────────────┐
│  Resource                                                │
│  ────────                                                │
│  • 是"数据"，不是"行为"                                  │
│  • 被动：响应查询和修改                                  │
│  • 无 Tick，不参与调度                                   │
│  • 可被多个 Subsystem 借用                               │
│                                                          │
│  e.g. SceneGraph, GpuScene, BindlessTable, AssetRegistry │
├──────────────────────────────────────────────────────────┤
│  Subsystem                                               │
│  ─────────                                               │
│  • 是"行为"，有逻辑和调度                                │
│  • 主动：每帧 Tick，响应事件                             │
│  • 通常为无状态或仅持小量状态                            │
│  • 通过 World 借用 Resource 和其他 Subsystem             │
│                                                          │
│  e.g. CameraSystem, RenderSystem, InputSystem            │
└──────────────────────────────────────────────────────────┘
```

**C++ 里 `World::GetResource<T>()` 和 `World::GetSubsystem<T>()` 应该是两套 API，表达不同寿命语义。**

---

## 📍 核心对象归属表

| 对象                  | 层级                   | 角色               | 访问方式                                 |
|----------------------|-----------------------|-------------------|------------------------------------------|
| **WindowManager**    | Platform              | 对象               | AppShell 持有，Platform 事件总线广播     |
| **GpuDevice**        | Platform              | 对象               | 构造时以 `&` 注入下层                    |
| **BindlessManager**  | Platform              | 对象（线程安全）    | 构造注入 Render World / AssetServer      |
| **GpuResourceCache** | Platform              | 对象               | AssetServer / GpuScene 借用              |
| **AssetServer**      | Platform              | 对象               | 跨 World 共享；World 持 Handle           |
| **Assets\<T\>**      | Platform              | 存储               | 通过 AssetServer 查询                    |
| **SurfaceRegistry**  | Platform              | 对象               | 订阅窗口事件；Render World 每帧 query    |
| **Ipc / Net Bus**    | Platform              | 对象               | 构造注入 ClientSystem                    |
| **SceneGraph**       | Main World            | 就是 ECS 本身      | Query / Actor API                        |
| **Viewport**         | Main World            | Resource (语义)     | `world.GetResource<Viewports>()`         |
| **AssetHandle\<T\>** | Main World            | ECS Component      | 通过组件查询                              |
| **WorldEventBus**    | Main World            | Resource            | 替代回调函数                              |
| **CameraController** | Main World            | Subsystem          | 读 InputState，写 Camera Transform       |
| **InputSystem**      | Main World            | Subsystem          | 读 Platform 输入事件                      |
| **ClientSystem**     | Main World            | Subsystem          | 持 `&IpcBus`（Platform 注入）            |
| **EntitySpawnSystem**| Main World            | Subsystem          | 处理 spawn/despawn 命令                  |
| **EntityRegistry**   | Main World            | Resource            | GUID ↔ Entity 映射                       |
| **GpuScene**         | Render World          | Resource            | Extract 写入，Render 读                   |
| **InstanceSlotAllocator** | Render World     | Resource (持久)     | Extract 分配/释放 slot                   |
| **ExtractedX**       | Render World          | Resource (per-frame)| Extract 阶段重建                         |
| **RenderGraph**      | Render World / Platform | 描述对象          | per-frame 编排或增量                      |
| **FrameContext**     | Render World          | per-frame 对象      | Acquire 创建，Present 清理                |

### 几个容易放错的对象

- **"EntitySubsystem"** → 误命名。Entity 管理就是 World 本身。真正的拆分是 `EntitySpawnSystem` + `EntityRegistry` (Resource) + `DeferredCommands`。
- **"SceneGraph" 作为独立 Resource** → 不需要。ECS 的 World 加上 `Parent` / `Children` / `Transform` 组件就是 Scene Graph。
- **"BindlessManager" 作为 Render World Resource** → 错。descriptor heap 寿命 = GPU Device 寿命，属于 Platform。async I/O 线程也要注册 texture，它必须跨 World、跨线程。
- **"GpuScene" 作为 Platform** → 错。GpuScene 持有 per-frame 动态数据，多 World (主视口+预览) 需要各一份，寿命跟 World 走。

---

## 🔄 Main World ↔ Render World：Actor/Slot 映射的稳定性

### 关键洞见

> **"Render World 只读 Main World" 说的是每帧数据流向，不是 Render World 不能有自己的持久状态。**  
> Slot 的稳定性恰恰来自 Render World **自己维护的 `Entity→Slot` 映射表**，这张表跨帧存活。

### 稳定性的三层复合

```
┌──────────────────────────────────────────────────────────────┐
│ Layer A: Entity ID 稳定                                      │
│   由 ECS 保证。despawn 前不变。                              │
├──────────────────────────────────────────────────────────────┤
│ Layer B: Slot 稳定 (关键)                                    │
│   由 Render World 的 InstanceSlotAllocator 保证              │
│   • 同 Entity 总映射到同 slot (幂等 alloc)                   │
│   • 只在 despawn 时 free                                     │
│   • free list 复用，控制 buffer 增长                         │
│   • deferred free（等 frames-in-flight 结束）                │
├──────────────────────────────────────────────────────────────┤
│ Layer C: Bindless Index 稳定                                 │
│   由 Platform 的 BindlessTable 保证                          │
│   • 资源上传即分配，unload 才释放                            │
│   • 跨 World 跨帧全局稳定                                    │
└──────────────────────────────────────────────────────────────┘
```

### Slot Allocator 结构

```cpp
// Render World 的持久 Resource
struct InstanceSlotAllocator {
    HashMap<Entity, InstanceSlot> entity_to_slot;
    Vector<InstanceSlot>          free_list;
    uint32_t                      high_water_mark = 0;
    
    InstanceSlot alloc_for(Entity e);   // 幂等：同 Entity 总返回同 slot
    void         free_for(Entity e);    // despawn 时才释放
};
```

### Extract 阶段：增量而非重建

```cpp
void extract_meshes(
    Query<(Entity, &Transform, &MeshHandle), Changed<Transform>> main_query,
    ResMut<InstanceSlotAllocator> allocator,
    ResMut<GpuScene> gpu_scene)
{
    for (auto [e, xform, mesh] : main_query) {
        auto slot = allocator.alloc_for(e);
        gpu_scene.mark_dirty(slot, xform, mesh);
    }
}

void extract_despawns(
    Events<EntityDespawned> events,
    ResMut<InstanceSlotAllocator> allocator,
    ResMut<GpuScene> gpu_scene)
{
    for (auto& e : events.iter()) {
        auto slot = allocator.free_for(e.entity);
        gpu_scene.mark_freed(slot);
    }
}
```

**每帧 Extract 不遍历所有 Actor**，只处理 `Changed<T>` filter 命中的；未变化的 Actor 的 slot 保持不动，GPU 数据稳定。

### 典型失败模式

| 反例                                | 后果                          | 正确做法                         |
|-----------------------------------|------------------------------|--------------------------------|
| 每帧 clear + rebuild InstanceBuffer | slot 跳变，motion vector 失效 | 保留 allocator，仅更新 dirty   |
| 按 visible 顺序紧凑打包 slot        | 相机转动就重排，所有索引失效  | 固定 slot，visible 通过 indirect count 控制 |
| 把 slot 号塞回 Main 的 Actor        | 违反 "Render 不写 Main"       | slot 只在 Render World 知道     |
| despawn 立即释放 slot               | GPU 还在用，读后写 UB         | deferred N frames 释放          |

### Slot 分配策略（按场景选）

| 方案 | 描述 | 适用 |
|------|------|------|
| A: 简单 free list | HashMap + Vec，O(1) alloc/free | Actor 数量稳定（起点推荐） |
| B: 分代 slab      | 固定 chunk，每 chunk 自己 freelist | 极多实体、流式加载 |
| C: Archetype 分桶 | 按 (mesh, material) 打包连续 slot | 有利合批；动态实体不适用 |

Bevy 采用 A + 合批在 queue 阶段排序。

---

## 🪟 Platform / Render World / Main World：窗口 & 渲染交互

### 三个关键判定

1. **窗口不是 Render World 的数据，是它的输出目标** —— Render World 借用 `SurfaceRegistry`，不持有窗口
2. **Swapchain 归 Platform** —— 订阅窗口事件，自动维护；Render World 每帧 query
3. **渲染的发起者是 Render World 自己的 Schedule** —— 不是相机、不是窗口、不是 App

### Swapchain 归属方案对比

| 方案                | 优点                                 | 缺点                                    | 选择     |
|-------------------|------------------------------------|---------------------------------------|--------|
| A: Platform 持有   | 职责清晰；多 World 共享窗口天然支持     | 需明确 present 语义                    | ✅ 推荐 |
| B: Render World 持有| GPU 资源集中                        | 多 Render World 混乱；窗口先于 Render 创建时尴尬 | ✗      |
| C: Window 对象内嵌  | "窗口即渲染目标"语义直观              | Window 层泄漏 GPU 依赖；headless 不自然 | ✗      |

### Viewport：Main 侧的语义层

Main World 不该有 `Swapchain` 概念，但需要表达"这个相机画到那个窗口"：

```cpp
// Main World Resource
struct Viewport {
    Entity       camera_entity;
    WindowId     target_window;   // 只是 id，不是 Window*
    float2       render_scale = 1;
    float4       clear_color;
    uint32_t     layer_mask;
};
```

Extract 阶段翻译成 Render World 的 `ExtractedViewport`：

```cpp
void extract_viewports(
    Res<Viewports> main_viewports,
    Res<SurfaceRegistry> surfaces,   // Platform 借用
    ResMut<ExtractedViewports> out)
{
    out.clear();
    for (auto& vp : main_viewports.list) {
        auto* surface = surfaces.get(vp.target_window);
        if (!surface) continue;      // 窗口可能已销毁，自然跳过
        out.push({
            .view_matrix = compute_view(vp.camera_entity),
            .proj_matrix = compute_proj(vp.camera_entity),
            .surface     = surface,
            .render_target_size = surface->size * vp.render_scale,
        });
    }
}
```

**带来的好处**：
- Main World 代码在**无 GPU** 的测试环境能跑
- 窗口销毁时自然跳过，不崩
- 截图/录屏只需加假 Viewport 指向 offscreen target

### 渲染 Schedule（Render World 的 tick）

```
1. Extract Phase
   从 Main World 拷贝 cameras/lights/meshes → ExtractedX

2. Acquire Phase
   for each ExtractedViewport:
     surface = platform.surfaces.get(vp.target_window)
     backbuffer = surface.acquire_next()
     → 写入 FrameContext

3. Prepare Phase
   上传 dirty instance / uniform
   pipeline compile
   transient resource allocation

4. Queue Phase
   culling / sorting / batching
   produce DrawCommands per RenderPhase

5. Render Phase
   RenderGraph.execute():
     for each pass:
       cmd.begin_render_pass(...)
       for each draw: cmd.draw(...)
       cmd.end_render_pass()

6. Submit Phase
   queue.submit(cmd_buffers, fences)

7. Present Phase
   for each used surface:
     surface.present(backbuffer)
```

### 每帧总体时序

```
shell.tick() {
  1. Platform: pump OS events
     WindowManager → InputEvents, WindowResized (SurfaceRegistry 处理)

  2. Main World.tick()
     InputSystem → CameraSystem → Transform/Anim/Physics...

  3. Render World.tick()
     Extract → Acquire → Prepare → Queue → Render → Submit → Present
     ↳ Acquire 时 get surface from Platform.SurfaceRegistry
     ↳ Record 时用 GpuDevice.cmd_pool
     ↳ Present 时调 surface.present()
}
```

### 窗口 Resize 的跨层协作

```
t=0  OS 发出 WM_SIZE
t=1  Platform WindowManager 捕获 → push WindowResized 到 PlatformEventBus
t=2  SurfaceRegistry (Platform) 订阅该事件 → 标记 swapchain dirty
     (不立即重建，等 in-flight 帧结束)
t=3  Render World 下一帧 Acquire Phase:
     surfaces.get(window_id) → 发现 dirty → 等 GPU fence → 
     destroy old → create new → 返回新 swapchain
t=4  Main World 同时也订阅 WindowResized → Viewport 更新 render_target_size
     → CameraSystem 更新投影矩阵 aspect ratio
t=5  渲染继续，两边都已就绪
```

**关键**：Main 和 Render 分别订阅**同一个 OS 事件**，但处理内容不同 —— 一个改语义，一个改物理。事件总线是跨层协调的胶水。

### 典型失败模式

| 反例 | 后果 | 正确做法 |
|------|------|---------|
| Main 的 Camera 组件持 Swapchain* | Main 污染 GPU 依赖；无头测试不可能 | Camera 只持 target_window_id |
| Render 过程回调 Main | 数据竞争 | Render 写 RenderEvents，Main 下一帧读 |
| Render World 处理 swapchain resize | 窗口线程 & 渲染线程竞争 | Platform 单线程处理，Render 每帧 query |
| 多 Viewport 共享 RenderGraph 实例 | 并行执行状态污染 | graph 描述共享不可变，FrameContext per-viewport |
| Command buffer 跨线程录制无同步 | Vk/D3D12 要求 cmd pool 单线程 | 每录制线程独立 pool，submit 汇总 |

---

## 🧪 架构健康度自检清单

```
✓ Subsystem 成员里找不到 App* / AppShell*
✓ Subsystem 成员里可以有 World* (这是上下文)
✓ Subsystem 成员里可以有 Platform 资源的 & (构造注入)
✓ Main World 的代码不 include 任何 CGPU / vulkan 头文件
✓ Render World 不写 Main World 的任何数据
✓ Platform 的类型不知道 World 的存在
✓ 没有 ::get() 形式的全局访问
✓ 多 World 是 "多开 Main World"，Platform 复用
✓ Resource 类型和 Subsystem 类型分开注册 (两个 registry)
✓ Slot 分配器是 Render World 的 Resource，跨帧存活
✓ 窗口 resize 由 Platform 订阅处理，不惊动 World 业务代码
```

---

## 💡 Bevy 几个值得借鉴的细节

1. **SystemParam**：Subsystem 的 tick 函数参数**声明式**表达依赖
   ```rust
   fn my_system(input: Res<InputState>, mut cam: Query<&mut Camera>)
   ```
   C++ 可用 tuple + concepts 模拟。

2. **Commands buffer**：避免 tick 中途修改 ECS 结构，批量 defer 到帧末。

3. **Schedule + SystemSet**：subsystem 之间顺序通过**图依赖**声明，不靠注册顺序。消除 race。

4. **Extract 是唯一过桥**：Main/Render 两个 World 可并行跑，只在每帧一个同步点交换数据。

5. **Plugin 组装**：一个 Plugin 打包多个 Resource + System + 依赖声明。适合映射到项目的模块系统。

6. **渲染是 pull 不是 push**：Main World 不"告诉" Render World 画，Render World 自己 tick、主动 pull。这让 Main/Render 能真正并行。

7. **Headless 是免费的**：把 SurfaceRegistry 换成产 offscreen target 的实现，Render 代码一行不改。这是"窗口作为输出目标"抽象的威力。

---

## 📌 常见反模式总结

| 反模式 | Rust 视角 | 修正方向 |
|-------|----------|---------|
| Subsystem 持 `App*` back-pointer | self-referential struct，Rust 编译器拒绝 | Subsystem 只持 `World*`，兄弟通信走 `World::RequireSubsystem` |
| 单例 `::get()` 全局访问 | 隐藏依赖，无法 mock | 构造时注入依赖，显式参数 |
| Subsystem 混存数据和行为 | 违反 SRP；难做 Extract/并行 | 拆成 Resource（数据）+ Subsystem（行为）|
| 把 GPU 类型塞进 Main World | 破坏 headless 能力 | Main 只存语义 (WindowId、Handle)，Extract 阶段翻译 |
| Render 回写 Main | 破坏单向流；产生数据竞争 | 用 RenderEvents，Main 下一帧消费 |
| Callback 成员函数字段 | 隐式全局 state | 事件总线 Resource |
| 每帧重建 GPU instance buffer | slot 跳变，TAA/motion vector 失效 | 持久 SlotAllocator + 增量 Extract |

---

## 🔚 回到开头的问题

> **App 作为上帝对象合理吗？**

不合理，是**架构级别**的问题（虽然不是编译错误级别）。当前能跑起来是因为 C++ 不强制声明对象图、单例"方便"、subsystem 生命周期粗略重合。但代价会随项目成长指数增长：并发场景越多越怕改状态；subsystem 越多 back-pointer 抓得越广；想抽出独立模块复用几乎不可能。

> **理想架构什么样？**

四层：**Platform / AppShell / Main World / Render World**。  
Platform 提供进程级资源；AppShell 只做胶水不存数据；Main World 跑 CPU 逻辑；Render World 跑 GPU。三层之间通过 **构造注入 + Extract + 事件总线** 协作，**永不反向**。

> **Subsystem 和 Resource 的区别？**

Subsystem 是**行为** (tick、响应事件)；Resource 是**数据** (被动查询)。当前项目最大的问题是把两者混在 `Subsystem` 这一个概念里。

> **Slot 稳定性怎么保证？**

Render World 维护 **`InstanceSlotAllocator` Resource** 跨帧存活，Entity→Slot 幂等映射，despawn 才释放（还要 deferred N frames）。"Render 只读 Main" 指数据流向，不指 Render 无状态。

> **窗口怎么到 Render？**

不"到"，是**被借用**。`SurfaceRegistry` 住在 Platform，订阅 OS 窗口事件、维护 Swapchain；Render World 每帧 `surfaces.get(window_id)` 借用；Main World 只持 `WindowId` 这个纯语义 id。

---

## 📚 延伸阅读

- Bevy ECS & Schedule: https://bevy-cheatbook.github.io/programming/ecs-intro.html
- Bevy Render Pipeline (Extract/Prepare/Queue/Render): https://bevy-cheatbook.github.io/gpu/intro.html
- Bevy 官方 `bevy_render::view::ExtractedView`、`bevy_pbr::render::MeshUniform` 的源码是最好的实证参考
