use crate::dlss_sr::DlssSrState;
use crate::frame_state::FrameRenderState;
use crate::frame_timing::FrameTiming;
use crate::gfx_resource_manager::GfxResourceManager;
use crate::per_frame_gpu_data::PerFrameGpuData;
use crate::render_options::RenderOptions;
use crate::shader_binding_system::ShaderBindingView;
use crate::view_accum::ViewAccumState;

/// pass 录制阶段的只读共享渲染上下文。
///
/// 该上下文只表达 pass 录制确实需要的 GPU-facing 状态。资源创建、bindless 注册、
/// resize 和 shutdown 仍通过对应生命周期 Ctx 的可变 owner 完成。
#[derive(Clone, Copy)]
pub struct RenderPassRecordCtx<'a> {
    pub frame_timing: &'a FrameTiming,
    pub frame_state: &'a FrameRenderState,
    pub render_options: &'a RenderOptions,
    pub view_accum: &'a ViewAccumState,
    pub dlss_sr_state: &'a DlssSrState,
    pub shader_bindings: ShaderBindingView<'a>,
    pub gfx_resource_manager: &'a GfxResourceManager,
    pub per_frame_gpu_data: &'a PerFrameGpuData,
}
