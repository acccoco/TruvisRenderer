use truvis_gfx::resources::special_buffers::structured_buffer::GfxStructuredBuffer;
use truvis_shader_binding::gpu;

use crate::bindless_manager::BindlessManager;
use crate::fif_buffer::FifBuffers;
use crate::frame_counter::FrameCounter;
use crate::gfx_resource_manager::GfxResourceManager;
use crate::global_descriptor_sets::GlobalDescriptorSets;
use crate::pipeline_settings::{AccumData, FrameSettings, PipelineSettings};
use crate::sampler_manager::RenderSamplerManager;

/// GPU-facing 共享资源与帧状态的聚合容器。
///
/// 持有 render pass 和 plugin 需要共享访问的 manager、descriptor、FIF 资源和 frame state，
/// 与 CPU 场景状态（`World`）物理分离。`GpuScene`、present、asset manager 和 bridge 仍由
/// `RenderRuntime` 私有持有。保持 plain struct（公开字段），利用 Rust 的 disjoint field
/// borrowing 避免 `&mut self` 方法导致的借用冲突。
pub struct GpuStore {
    pub bindless_manager: BindlessManager,
    pub global_descriptor_sets: GlobalDescriptorSets,
    pub gfx_resource_manager: GfxResourceManager,
    pub fif_buffers: FifBuffers,
    pub sampler_manager: RenderSamplerManager,
    pub per_frame_data_buffers: [GfxStructuredBuffer<gpu::PerFrameData>; FrameCounter::fif_count()],

    pub frame_counter: FrameCounter,
    pub frame_settings: FrameSettings,
    pub pipeline_settings: PipelineSettings,

    pub delta_time_s: f32,
    pub total_time_s: f32,
    pub accum_data: AccumData,
}
