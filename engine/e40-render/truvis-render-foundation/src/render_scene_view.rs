use ash::vk;

use truvis_gfx::commands::command_buffer::GfxCommandBuffer;

use crate::frame_counter::FrameLabel;

/// 影响离线 progressive accumulation 是否可以继续复用历史结果的场景签名。
///
/// 这里只暴露版本号，不暴露具体 GPU scene owner。调用方可以把它和相机、离线设置组合，
/// 在 scene/light/material/sky 语义变化时清空 reference 累计结果。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RenderSceneAccumSignature {
    /// TLAS revision 覆盖 instance 集合、几何引用和 transform 变化；这些变化会直接改变 primary/secondary ray 命中。
    pub tlas_revision: u64,
    /// 自发光三角形表、alias table 或相关材质/mesh 语义变化会改变 emissive NEE 和 hit emission。
    pub emissive_light_version: u32,
    /// analytic light 列表或参数变化会改变 direct lighting sample，因此必须让离线累计失效。
    pub analytic_light_version: u32,
    /// sky 分布版本覆盖 HDRI importance table 与 fallback/真实贴图切换；它影响 sky radiance 与 PDF。
    pub sky_distribution_version: u32,
}

/// Render pass 访问 GPU scene 的最小只读契约。
///
/// concrete `RenderWorld` 和场景上传数据属于 render-backend，pass 只通过这里
/// 读取 shader 可见根 buffer、TLAS handle，并提交光栅化 draw。
pub trait RenderSceneView {
    fn scene_buffer_device_address(&self, frame_label: FrameLabel) -> vk::DeviceAddress;

    fn tlas_handle(&self, frame_label: FrameLabel) -> Option<vk::AccelerationStructureKHR>;

    fn accum_signature(&self, frame_label: FrameLabel) -> RenderSceneAccumSignature;

    fn draw_raster(&self, frame_label: FrameLabel, cmd: &GfxCommandBuffer, before_draw: &mut dyn FnMut(u32, u32));
}
