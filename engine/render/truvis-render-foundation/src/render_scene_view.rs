use ash::vk;

use truvis_gfx::commands::command_buffer::GfxCommandBuffer;

use crate::frame_counter::FrameLabel;

/// Render pass 访问 GPU scene 的最小只读契约。
///
/// concrete `GpuScene` 和场景上传数据属于 render-backend，pass 只通过这里
/// 读取 shader 可见根 buffer、TLAS handle，并提交光栅化 draw。
pub trait RenderSceneView {
    fn scene_buffer_device_address(&self, frame_label: FrameLabel) -> vk::DeviceAddress;

    fn tlas_handle(&self, frame_label: FrameLabel) -> Option<vk::AccelerationStructureKHR>;

    fn draw_raster(&self, frame_label: FrameLabel, cmd: &GfxCommandBuffer, before_draw: &mut dyn FnMut(u32, u32));
}
