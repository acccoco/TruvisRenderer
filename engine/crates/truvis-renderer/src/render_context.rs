//! 渲染上下文与 Phase 模型
//!
//! 每帧的渲染流程分为若干 Phase，每个 Phase 有严格的数据读写规则：
//!
//! | Phase            | CPU 数据   | GPU 数据   | 典型操作                        |
//! |------------------|-----------|-----------|--------------------------------|
//! | CPU Update       | 读写      | —         | SceneManager 更新场景           |
//! | GPU Upload       | 只读      | 写入      | prepare_render_data, upload     |
//! | Render           | 只读      | 只读/写入  | Pass 执行，RenderGraph dispatch |
//!
//! 这样做的好处：
//! - 明确数据流向，避免资源访问冲突
//! - 将同阶段的数据组装在一起（如 `RenderContext2<'a>`），简化接口设计
//!
//! `RenderContext` 持有全部渲染资源的所有权，在 GPU Upload 阶段可变访问；
//! `RenderContext2<'a>` 是其只读借用切片，用于 Render 阶段，通过类型系统强制不可变。

use truvis_asset::asset_hub::AssetHub;
use truvis_gfx::resources::special_buffers::structured_buffer::GfxStructuredBuffer;
use truvis_render_graph::resources::fif_buffer::FifBuffers;
use truvis_render_interface::bindless_manager::BindlessManager;
use truvis_render_interface::frame_counter::FrameCounter;
use truvis_render_interface::gfx_resource_manager::GfxResourceManager;
use truvis_render_interface::global_descriptor_sets::GlobalDescriptorSets;
use truvis_render_interface::gpu_scene::GpuScene;
use truvis_render_interface::pipeline_settings::{AccumData, FrameSettings, PipelineSettings};
use truvis_render_interface::sampler_manager::RenderSamplerManager;
use truvis_scene::scene_manager::SceneManager;
use truvis_shader_binding::gpu;

/// 渲染上下文，持有一帧渲染所需全部资源的所有权。
///
/// 在 GPU Upload 阶段以 `&mut self` 访问（prepare / upload），
/// 在 Render 阶段通过 [`RenderContext2`] 以只读方式暴露给各 Pass。
pub struct RenderContext {
    pub scene_manager: SceneManager,
    pub gpu_scene: GpuScene,
    pub asset_hub: AssetHub,

    pub fif_buffers: FifBuffers,
    pub bindless_manager: BindlessManager,
    pub per_frame_data_buffers: [GfxStructuredBuffer<gpu::PerFrameData>; FrameCounter::fif_count()],
    pub gfx_resource_manager: GfxResourceManager,
    pub sampler_manager: RenderSamplerManager,

    pub global_descriptor_sets: GlobalDescriptorSets,

    pub delta_time_s: f32,
    pub total_time_s: f32,
    pub accum_data: AccumData,

    pub frame_counter: FrameCounter,
    pub frame_settings: FrameSettings,
    pub pipeline_settings: PipelineSettings,
}

/// Render 阶段的只读上下文切片。
///
/// 通过生命周期 `'a` 借用 [`RenderContext`] 的资源，
/// 利用类型系统保证 Render Phase 中数据不被意外修改。
#[derive(Copy, Clone)]
pub struct RenderContext2<'a> {
    pub scene_manager: &'a SceneManager,
    pub gpu_scene: &'a GpuScene,
    pub asset_hub: &'a AssetHub,

    pub fif_buffers: &'a FifBuffers,
    pub bindless_manager: &'a BindlessManager,
    pub per_frame_data_buffers: &'a [GfxStructuredBuffer<gpu::PerFrameData>; FrameCounter::fif_count()],
    pub gfx_resource_manager: &'a GfxResourceManager,
    pub sampler_manager: &'a RenderSamplerManager,

    pub global_descriptor_sets: &'a GlobalDescriptorSets,

    pub delta_time_s: f32,
    pub total_time_s: f32,
    pub accum_data: AccumData,

    pub frame_counter: &'a FrameCounter,
    pub frame_settings: &'a FrameSettings,
    pub pipeline_settings: &'a PipelineSettings,
}
