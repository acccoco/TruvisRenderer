## ADDED Requirements

### Requirement: RenderWorld 定义在 truvis-render-interface 中

`RenderWorld` 结构体 SHALL 定义在 `truvis-render-interface` crate 的 `render_world` 模块中，持有全部 GPU 渲染状态和帧状态。

#### Scenario: RenderWorld 的字段组成

- **WHEN** 查看 `RenderWorld` 结构体定义
- **THEN** 其字段 SHALL 包含：`gpu_scene: GpuScene`、`bindless_manager: BindlessManager`、`global_descriptor_sets: GlobalDescriptorSets`、`gfx_resource_manager: GfxResourceManager`、`fif_buffers: FifBuffers`、`sampler_manager: RenderSamplerManager`、`per_frame_data_buffers`、`frame_counter: FrameCounter`、`frame_settings: FrameSettings`、`pipeline_settings: PipelineSettings`、`delta_time_s: f32`、`total_time_s: f32`、`accum_data: AccumData`

#### Scenario: RenderWorld 不包含 CPU 场景数据

- **WHEN** 查看 `RenderWorld` 结构体定义
- **THEN** SHALL NOT 包含 `SceneManager` 或 `AssetHub` 类型的字段

#### Scenario: RenderWorld 从 render-interface 导入

- **WHEN** 任何 crate 需要使用 `RenderWorld` 类型
- **THEN** SHALL 从 `truvis_render_interface::render_world::RenderWorld` 导入

### Requirement: World 定义在 truvis-world crate 中

`World` 结构体 SHALL 定义在独立的 `truvis-world` crate 中，持有 CPU 侧场景状态。

#### Scenario: World 的字段组成

- **WHEN** 查看 `World` 结构体定义
- **THEN** 其字段 SHALL 包含：`scene_manager: SceneManager`、`asset_hub: AssetHub`

#### Scenario: truvis-world 的依赖

- **WHEN** 检查 `truvis-world` 的 `Cargo.toml`
- **THEN** workspace 依赖 SHALL 仅包含 `truvis-scene` 和 `truvis-asset`（及其传递依赖所需的 crate）

#### Scenario: World 从 truvis-world 导入

- **WHEN** 任何 crate 需要使用 `World` 类型
- **THEN** SHALL 从 `truvis_world::World` 导入

### Requirement: FifBuffers 定义在 truvis-render-interface 中

`FifBuffers` SHALL 从 `truvis-render-graph` 迁移到 `truvis-render-interface` crate。

#### Scenario: FifBuffers 在 render-interface 中可用

- **WHEN** 运行 `cargo check -p truvis-render-interface`
- **THEN** 编译成功且 `FifBuffers` 类型可从 `truvis_render_interface` 导出

#### Scenario: render-graph 不再定义 FifBuffers

- **WHEN** 检查 `truvis-render-graph` 的源码
- **THEN** SHALL NOT 包含 `FifBuffers` 的定义（re-export 用于过渡期的除外）

### Requirement: Renderer 持有 World 和 RenderWorld

`Renderer` 结构体 SHALL 持有 `World` 和 `RenderWorld` 作为独立字段，替代原有的 `RenderContext`。

#### Scenario: Renderer 的字段结构

- **WHEN** 查看 `Renderer` 结构体定义
- **THEN** SHALL 包含 `pub world: World` 和 `pub render_world: RenderWorld` 字段
- **AND** SHALL NOT 包含 `render_context: RenderContext` 字段

#### Scenario: RenderContext 和 RenderContext2 被删除

- **WHEN** 搜索 workspace 中的 `RenderContext` 结构体定义
- **THEN** SHALL NOT 存在 `RenderContext` 或 `RenderContext2` 的 struct 定义

### Requirement: AppPlugin 的 RenderCtx 使用 RenderWorld

`RenderCtx` SHALL 通过 `&RenderWorld` 提供 GPU 状态访问，替代原有的 `&RenderContext`。

#### Scenario: RenderCtx 字段

- **WHEN** 查看 `RenderCtx` 结构体定义
- **THEN** SHALL 包含 `pub render_world: &'a RenderWorld` 字段
- **AND** SHALL NOT 包含 `render_context` 字段

#### Scenario: Plugin render 阶段只能读取 GPU 状态

- **WHEN** AppPlugin 的 `render` 方法被调用
- **THEN** 通过 `RenderCtx` 只能获得 `&RenderWorld`（不可变引用），无法修改 GPU 状态

### Requirement: AppPlugin 的 UpdateCtx 使用 World

`UpdateCtx` SHALL 通过 `&mut World` 提供 CPU 状态访问。

#### Scenario: UpdateCtx 字段

- **WHEN** 查看 `UpdateCtx` 结构体定义
- **THEN** SHALL 包含 `pub world: &'a mut World` 字段
- **AND** SHALL 包含 `pub pipeline_settings: &'a mut PipelineSettings` 和 `pub frame_settings: &'a FrameSettings`
- **AND** SHALL NOT 包含 `scene_manager` 或 `render_context` 字段

### Requirement: AppPlugin 的 InitCtx 同时使用 World 和 RenderWorld

`InitCtx` SHALL 提供对 `World` 和 `RenderWorld` 的可变访问。

#### Scenario: InitCtx 字段

- **WHEN** 查看 `InitCtx` 结构体定义
- **THEN** SHALL 包含 `pub world: &'a mut World` 和 `pub render_world: &'a mut RenderWorld`
- **AND** SHALL NOT 包含单独的 `scene_manager`、`asset_hub`、`bindless_manager`、`gfx_resource_manager`、`global_descriptor_sets` 字段

### Requirement: AppPlugin 的 ResizeCtx 使用 RenderWorld

`ResizeCtx` SHALL 通过 `&mut RenderWorld` 提供 GPU 状态的可变访问。

#### Scenario: ResizeCtx 字段

- **WHEN** 查看 `ResizeCtx` 结构体定义
- **THEN** SHALL 包含 `pub render_world: &'a mut RenderWorld` 字段
- **AND** SHALL NOT 包含单独的 `frame_settings`、`global_descriptor_sets`、`gfx_resource_manager`、`bindless_manager` 字段

### Requirement: Render passes 使用 RenderWorld 而非 RenderContext

所有 render graph pass 的 RgPass 适配器 SHALL 使用 `&RenderWorld` 替代 `&RenderContext`。

#### Scenario: RgPass 适配器的字段

- **WHEN** 查看 `SdrRgPass`、`BlitRgPass`、`ResolveRgPass`、`RealtimeRtRgPass`、`DenoiseAccumRgPass`、`AccumRgPass`、`GuiRgPass` 的结构体定义
- **THEN** SHALL 包含 `pub render_world: &'a RenderWorld` 字段
- **AND** SHALL NOT 包含 `render_context` 字段

#### Scenario: truvis-render-passes 不依赖 truvis-renderer

- **WHEN** 检查 `truvis-render-passes` 的 `Cargo.toml`
- **THEN** SHALL NOT 包含对 `truvis-renderer` 的依赖

### Requirement: 编译通过且功能不回归

#### Scenario: Workspace 编译通过

- **WHEN** 运行 `cargo check --workspace`
- **THEN** 编译成功无错误

#### Scenario: 现有 demo 程序可运行

- **WHEN** 运行 triangle、shader-toy、rt-cornell、rt-sponza 任一 demo
- **THEN** 渲染输出与变更前一致
