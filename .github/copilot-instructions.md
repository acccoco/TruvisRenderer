# Render-Rust-vk-Truvis Copilot 指令

基于 Rust 和 Vulkan 1.3+ 的现代渲染引擎，支持 Slang 自动着色器绑定和硬件光线追踪。

## 🏗️ 核心架构

```
engine/
├── crates/
│   ├── truvis-gfx/              # Vulkan RHI 抽象（Gfx 单例）
│   ├── truvis-render-interface/ # FrameCounter, CmdAllocator, GfxResourceManager, Handle 系统
│   ├── truvis-render-graph/     # RenderGraphBuilder, RenderContext, FifBuffers
│   ├── truvis-renderer/         # Renderer 核心，Camera, Timer, RenderPresent
│   ├── truvis-app/              # OuterApp trait，内置应用实现
│   ├── truvis-scene/            # 几何体、场景数据
│   ├── truvis-shader/           # 着色器编译（truvis-shader-build）与绑定生成（truvis-shader-binding）
│   ├── truvis-cxx/              # C++ FFI（Assimp 场景加载）
│   ├── truvis-gui-backend/      # ImGui Vulkan 后端（GuiPass）
│   ├── truvis-utils/            # 通用工具
│   └── truvis-asset/            # 异步资产加载
├── shader/
│   ├── entry/                   # .slang 入口着色器（按 pass 组织）
│   ├── share/                   # 共享头文件（.slangi）：结构体、全局绑定
│   └── .build/                  # 编译后 .spv（自动生成）
└── cxx/                         # C++ 源码 + CMakeLists.txt

truvis-crate-tools/              # 独立 crate：TruvisPath 路径工具
truvis-winit-app/
├── src/bin/                     # 应用入口点
└── src/app.rs                   # WinitApp 窗口管理
```

**依赖层次**: `truvis-gfx` → `truvis-render-interface` → `truvis-render-graph` → `truvis-renderer` → `truvis-app` → `truvis-winit-app`

## 🚀 构建流程（必须按顺序）

```powershell
# 1. 拉取资源和工具（首次克隆后）
cargo run --bin fetch_res

# 2. 构建 C++ 模块
cargo run --bin cxx-build

# 3. 编译着色器（运行前必需！）
cargo run --bin shader-build

# 4. 项目构建
cargo build --all

# 5. 运行演示
cargo run --bin triangle          # 基础三角形
cargo run --bin rt-cornell        # Cornell Box 光追
cargo run --bin rt-sponza         # Sponza 光追场景
cargo run --bin shader-toy        # 着色器实验场
```

**⚠️ 关键约束**:
- `shader-build` 必须在运行任何渲染应用前执行
- 着色器使用 rayon 并行编译 `.slang` → `.spv`，输出到 `engine/shader/.build/`

**自动生成系统**:
- 着色器绑定: `engine/crates/truvis-shader/truvis-shader-binding/build.rs` 从 `engine/shader/share/*.slangi` 生成 Rust 类型
- C++ 绑定: `truvis-cxx/build.rs` 构建 CMake 并复制 DLL 到 `target/`


## 🎯 OuterApp 开发模式

应用入口位于 `truvis-winit-app/src/bin/`，OuterApp 实现位于 `truvis-app/src/outer_app/`。

**线程模型**: winit 主线程只做事件 pump；`WinitApp::run` 在渲染线程中通过工厂闭包构造 `OuterApp`，后续 `init` / `update` / `draw` / `draw_ui` 全部在渲染线程中调用。因此传入的闭包必须 `Send + 'static`，且 OuterApp 内部可以使用 `Rc` 等线程局部类型（只要不跨线程泄露引用）。

```rust
// truvis-winit-app/src/bin/my_app.rs
use truvis_app::outer_app::my_app::MyAppImpl;
use truvis_winit_app::app::WinitApp;

fn main() {
    // 工厂闭包在渲染线程上调用一次；OuterApp 整个生命周期都在渲染线程
    WinitApp::run(|| Box::new(MyAppImpl::default()));
}

// truvis-app/src/outer_app/my_app.rs
#[derive(Default)]
pub struct MyAppImpl {
    triangle_pass: Option<TrianglePass>,
    gui_pass: Option<GuiPass>,
    cmds: Vec<GfxCommandBuffer>,
}
impl OuterApp for MyAppImpl {
    fn init(&mut self, renderer: &mut Renderer, _camera: &mut Camera) {
        self.triangle_pass = Some(TrianglePass::new(renderer.swapchain_image_info().image_format));
        self.gui_pass = Some(GuiPass::new(...));
        // 每帧预分配 CommandBuffer
        self.cmds = FrameCounter::frame_labes()
            .iter()
            .map(|label| renderer.cmd_allocator.alloc_command_buffer(*label, "my-app"))
            .collect_vec();
    }
    fn draw(&self, renderer: &Renderer, gui_draw_data: &imgui::DrawData, fence: &GfxSemaphore) {
        // 使用 RenderGraph 构建渲染流程
        let mut graph = RenderGraphBuilder::new();
        graph.add_pass_lambda("my-pass", |builder| { ... }, |context| { ... });
        let compiled = graph.compile();
        compiled.execute(&cmd, &renderer.render_context.gfx_resource_manager);
    }
    fn draw_ui(&mut self, _ui: &imgui::Ui) { /* ImGui 绘制 */ }
    fn update(&mut self, _renderer: &mut Renderer) { /* 每帧逻辑 */ }
    fn on_window_resized(&mut self, _renderer: &mut Renderer) { /* 窗口重建 */ }
}
```

### RenderGraph 模式

使用声明式 RenderGraph 构建渲染流程，自动处理图像屏障和信号量同步：

```rust
let mut graph = RenderGraphBuilder::new();

// 信号量同步（timeline semaphore）
graph.signal_semaphore(RgSemaphoreInfo::timeline(fence.handle(), vk::PipelineStageFlags2::BOTTOM_OF_PIPE, frame_id));

// 导入外部资源
let swapchain_rg = graph.import_image("swapchain", image_handle, Some(view_handle), format, 
    RgImageState::UNDEFINED_BOTTOM,
    Some(RgSemaphoreInfo::binary(present_semaphore.handle(), vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT)));

// 导出资源状态
graph.export_image(swapchain_rg, RgImageState::PRESENT_BOTTOM, Some(RgSemaphoreInfo::binary(...)));

// 添加渲染 Pass（lambda 或 trait 实现）
graph.add_pass_lambda("my-pass", |builder| {
    builder.read_write_image(swapchain_rg, RgImageState::COLOR_ATTACHMENT_READ_WRITE);
}, |context| {
    let view = context.get_image_view(swapchain_rg).unwrap();
    // 绘制逻辑
});

// 编译并执行
let compiled = graph.compile();
compiled.execute(&cmd, &gfx_resource_manager);
```


## 🎨 着色器开发（Slang 优先）

### 目录结构
| 目录 | 用途 |
|------|------|
| `engine/shader/share/` | 共享头文件（`.slangi`）：结构体、全局绑定 |
| `engine/shader/entry/<pass>/` | 按渲染通道组织的着色器入口源码 |
| `engine/shader/.build/` | 编译输出（SPIR-V） |

### 全局描述符布局（三层绑定）
定义于 `engine/shader/share/global_binding_sets.slangi`：
```slang
// set 0: 全局采样器
[[vk::binding(0, 0)]] SamplerState global_samplers[];
// set 1: Bindless 资源（需 NonUniformResourceIndex）
[[vk::binding(0, 1)]] Sampler2D<float4> bindless_textures[];
// set 2: 每帧数据
[[vk::binding(0, 2)]] ConstantBuffer<PerFrameData> per_frame_data;
```

### Slang → Rust 自动绑定
```slang
// engine/shader/share/frame_data.slangi
struct PerFrameData { float4x4 projection; float4x4 view; float3 camera_pos; uint time_ms; };
```
```rust
// 自动生成到 truvis-shader-binding crate
use truvis_shader_binding::gpu::PerFrameData;
```

### 描述符布局宏
```rust
#[shader_layout]  // 来自 truvis-descriptor-layout-macro
struct MyLayout {
    #[binding = 0] uniforms: PerFrameData,
    #[texture(binding = 1)] diffuse: TextureHandle,
}
```


## 📁 资源管理

### TruvisPath
```rust
use truvis_path::TruvisPath;

let model = TruvisPath::assets_path("sponza.fbx");                      // assets/...
let texture = TruvisPath::resources_path("uv_checker.png");             // resources/...
let shader = TruvisPath::shader_build_path_str("rt/raygen.slang");      // shader/.build/...spv
// 注意：shader_build_path_str 自动添加 .spv 后缀
```

### 顶点数据
```rust
use truvis_scene::shapes::triangle::TriangleSoA;
use truvis_scene::components::geometry::RtGeometry;
let triangle: RtGeometry = TriangleSoA::create_mesh();  // 内置几何体
```

## 📐 关键约定

### 坐标系统（严格遵循）
- **模型/世界**: 右手，Y-Up
- **视图**: 右手，Y-Up，相机朝向 -Z
- **NDC**: 左手，Y-Up（Vulkan 标准）
- **帧缓冲**: 原点左上角，视口 `height < 0`（Y 轴翻转）

**Blender 导出设置**: Forward=Y, Up=Z

### 调试命名规范
```rust
// 格式: [frame-label]name
cmd.begin(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT, "ray-tracing");
// 帧计数器：render_context.frame_counter.frame_id
```

### 运行时控制
- **WASD**: 相机移动 | **鼠标**: 旋转 | **Shift**: 加速 | **F**: 切换 GUI

## 🔧 开发任务模板

### 添加新应用
```powershell
# 1. 在 truvis-app/src/outer_app/ 创建 OuterApp 实现
# 2. 在 truvis-winit-app/src/bin/ 创建入口文件 my_app.rs
# 3. 如需新着色器，在 engine/shader/src/ 添加 .slang 文件
# 4. 运行构建流程
cargo run --bin shader-build
cargo run --bin my_app
```

参考示例：`truvis-winit-app/src/bin/triangle_app.rs` + `engine/crates/truvis-app/src/outer_app/triangle/`

`OuterApp` trait 定义于 `engine/crates/truvis-app/src/outer_app/base.rs`

### 创建新渲染管线
```rust
// engine/crates/truvis-app/src/outer_app/my_app/my_subpass.rs
pub struct MySubpass {
    pipeline: GfxGraphicsPipeline,
    pipeline_layout: Rc<GfxPipelineLayout>,
}
impl RenderSubpass for MySubpass {}

// engine/crates/truvis-app/src/outer_app/my_app/my_pass.rs
impl MyPass {
    pub fn render(&self, render_context: &RenderContext, geometry: &RtGeometry) {
        let frame_label = render_context.frame_counter.frame_label();
        let cmd = self.cmds[*frame_label].clone();
        cmd.begin(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT, "my-pass");
        
        // 图像屏障
        cmd.image_memory_barrier(vk::DependencyFlags::empty(), &[/* barriers */]);
        
        // 绘制
        self.subpass.draw(&cmd, /* params */);
        
        cmd.end();
        Gfx::get().gfx_queue().submit(vec![GfxSubmitInfo::new(&[cmd])], None);
    }
}
```

### 集成新 C++ 库
参考 `engine/crates/truvis-cxx/build.rs` 的 CMake + DLL 复制模式：
```rust
// build.rs
println!("cargo:rustc-link-search=native={}", cargo_build_dir.display());
println!("cargo:rustc-link-lib=static=my-lib");
```

## 💡 关键实现细节

### Gfx 单例模式
```rust
// Gfx: 底层 Vulkan 抽象单例
Gfx::init("Truvis".to_string(), extra_instance_ext);
Gfx::get().gfx_device()  // 访问设备
Gfx::get().gfx_queue()   // 访问队列

// Renderer 管理整个渲染流程
// RenderContext 包含渲染状态，通过 renderer.render_context 访问

// 销毁顺序（在 Renderer::destroy() 中自动处理）
```

### Frames in Flight (FIF) 模式
- **固定 3 帧**: FrameLabel::A/B/C（`fif_count = 3`）
- **Timeline Semaphore**: 同步 GPU 进度（`frame_id` 与 semaphore value 对应）
- **FifBuffers**: 管理 render target、depth、color images

```rust
let frame_label = render_context.frame_counter.frame_label();  // A/B/C
let render_target = render_context.fif_buffers.render_target_image(frame_label);
```

### GfxResourceManager（Handle 系统）
使用 SlotMap 存储 GPU 资源，返回轻量级 Handle：
```rust
// 创建资源并获取 Handle
let image_handle: GfxImageHandle = resource_manager.create_image(create_info);
let view_handle: GfxImageViewHandle = resource_manager.get_or_create_image_view(image_handle, desc);

// 访问资源
let image = resource_manager.get_image(image_handle).unwrap();

// 延迟销毁（FIF 安全，cleanup() 在帧结束自动处理）
resource_manager.destroy_image_later(image_handle, frame_id);
```


## ⚠️ 关键限制和已知问题

### 构建依赖（必须按顺序执行）
```powershell
# ❌ 错误：直接运行会失败，因为着色器未编译
cargo run --bin triangle

# ✅ 正确：必须先编译着色器
cargo run --bin shader-build
cargo run --bin triangle
```

### 平台特定要求
- **Windows**: 需要 Visual Studio 2019+，vcpkg 自动通过 `vcpkg.json` 管理 Assimp
- **DLL 自动复制**: `truvis-cxx/build.rs` 自动复制 Assimp DLL 到 `target/debug|release/`
- **Vulkan SDK**: 必需 1.3+，`tools/slang/` 包含 Slang 编译器


## ⚠️ 常见陷阱

```rust
// ❌ 错误：忘记使用 TruvisPath
let shader = "shader/src/triangle/triangle.slang.spv";
// ✅ 正确：使用 shader_build_path_str（自动添加 .spv 后缀）
let shader = TruvisPath::shader_build_path_str("hello_triangle/triangle.slang");

// ❌ 错误：viewport 设置
let viewport = vk::Viewport { height: extent.height as f32, .. };
// ✅ 正确：Y轴翻转（Vulkan Y-down → 右手坐标系 Y-up）
let viewport = vk::Viewport { 
    y: extent.height as f32,
    height: -(extent.height as f32),
    ..
};

// ❌ 错误：OuterApp::draw() 签名（必须包含所有参数）
fn draw(&self) { }
// ✅ 正确：当前版本接收 Renderer、DrawData 和 fence
fn draw(&self, renderer: &Renderer, gui_draw_data: &imgui::DrawData, fence: &GfxSemaphore) { ... }

// ❌ 错误：直接访问 render_context
let ctx = renderer.render_context;  // 无法移动
// ✅ 正确：借用访问
let frame_label = renderer.render_context.frame_counter.frame_label();
```