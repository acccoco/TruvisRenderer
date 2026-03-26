use crate::resources::fif_buffer::FifBuffers;
use truvis_asset::asset_hub::AssetHub;
use truvis_gfx::resources::special_buffers::structured_buffer::GfxStructuredBuffer;
use truvis_render_interface::bindless_manager::BindlessManager;
use truvis_render_interface::frame_counter::FrameCounter;
use truvis_render_interface::gfx_resource_manager::GfxResourceManager;
use truvis_render_interface::global_descriptor_sets::GlobalDescriptorSets;
use truvis_render_interface::gpu_scene::GpuScene;
use truvis_render_interface::pipeline_settings::{AccumData, FrameSettings, PipelineSettings};
use truvis_render_interface::sampler_manager::RenderSamplerManager;
use truvis_scene::scene_manager::SceneManager;
use truvis_shader_binding::gpu;

// Render 期间不可变
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

/// 使用 <'a>，表示这些资源是临时的，可以被消费的，是对外展示的一个切片
///
/// 通过类型系统来表达架构意图，区分 Render 期间不可变的资源和可变的资源
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
