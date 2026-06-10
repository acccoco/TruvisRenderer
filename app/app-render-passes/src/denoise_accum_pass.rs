use ash::vk;

use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_gfx::gfx::GfxDeviceCtx;
use truvis_path::TruvisPath;
use truvis_render_graph::render_graph::{RgImageHandle, RgImageState, RgPass, RgPassBuilder, RgPassContext};
use truvis_render_runtime::bindings::bindless_manager::BindlessUavHandle;
use truvis_render_runtime::bindings::global_descriptor_sets::GlobalDescriptorSets;
use truvis_render_runtime::render_runtime_ctx::RenderPassRecordCtx;
use truvis_shader_binding::gpu;

use crate::compute_pass::ComputePass;

/// 降噪累积 Pass 的数据
pub struct DenoiseAccumPassData {
    pub single_frame_bindless_uav_handle: BindlessUavHandle,
    pub accum_bindless_uav_handle: BindlessUavHandle,
    pub gbuffer_a_bindless_uav_handle: BindlessUavHandle,
    pub gbuffer_b_bindless_uav_handle: BindlessUavHandle,
    pub gbuffer_c_bindless_uav_handle: BindlessUavHandle,
    pub image_size: vk::Extent2D,
    pub accum_frames: u32,
    pub denoise_enabled: bool,
    pub sigma_color: f32,
    pub sigma_depth: f32,
    pub sigma_normal: f32,
    pub kernel_radius: i32,
    /// 传统 denoise/accum pass-local 调试通道。
    ///
    /// 该字段只服务显式重新接入此 pass 的实验路径；主 RT debug UI 不再暴露
    /// legacy “禁用累积”通道。
    pub channel: u32,

    // 增强联合双边滤波参数
    pub sigma_albedo: f32,
    pub sigma_position: f32,
    pub scene_scale: f32,

    // 粗糙度自适应参数
    pub roughness_adaptive_enabled: bool,
    pub roughness_radius_scale: f32,
    pub roughness_sigma_scale: f32,
}

/// 保留的传统降噪累积 pass 默认参数。
///
/// 当前 RT 主流程已经旁路该 pass；这里的默认值只服务显式重新接入此 pass 的实验路径，
/// 不再通过 engine 全局 record ctx 暴露为项目级配置。
#[derive(Copy, Clone)]
pub struct DenoiseAccumSettings {
    pub enabled: bool,
    pub sigma_color: f32,
    pub sigma_depth: f32,
    pub sigma_normal: f32,
    pub kernel_radius: i32,
    pub sigma_albedo: f32,
    pub sigma_position: f32,
    pub scene_scale: f32,
    pub roughness_adaptive_enabled: bool,
    pub roughness_radius_scale: f32,
    pub roughness_sigma_scale: f32,
}

impl Default for DenoiseAccumSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            sigma_color: 0.1,
            sigma_depth: 1.0,
            sigma_normal: 0.5,
            kernel_radius: 3,
            sigma_albedo: 0.1,
            sigma_position: 0.1,
            scene_scale: 400.0,
            roughness_adaptive_enabled: true,
            roughness_radius_scale: 2.0,
            roughness_sigma_scale: 1.5,
        }
    }
}

/// 降噪累积 Pass - 对单帧 RT 结果进行双边滤波降噪，然后累积到 accum_image 中
pub struct DenoiseAccumPass {
    denoise_accum_pass: ComputePass<gpu::denoise_accum::PushConstant>,
}

impl DenoiseAccumPass {
    pub fn new(ctx: GfxDeviceCtx<'_>, render_descriptor_sets: &GlobalDescriptorSets) -> Self {
        let denoise_accum_pass = ComputePass::<gpu::denoise_accum::PushConstant>::new(
            ctx,
            render_descriptor_sets,
            c"main",
            TruvisPath::shader_build_path_str("pp/denoise_accum.slang").as_str(),
        );

        Self { denoise_accum_pass }
    }

    pub fn destroy(self, ctx: GfxDeviceCtx<'_>) {
        self.denoise_accum_pass.destroy(ctx);
    }

    pub fn exec(&self, cmd: &GfxCommandBuffer, data: DenoiseAccumPassData, record_ctx: &RenderPassRecordCtx<'_>) {
        let frame_label = record_ctx.frame_timing.frame_label();
        self.denoise_accum_pass.exec(
            cmd,
            frame_label,
            record_ctx.shader_bindings.global_descriptor_sets(),
            &gpu::denoise_accum::PushConstant {
                single_frame_input: data.single_frame_bindless_uav_handle.0,
                accum_output: data.accum_bindless_uav_handle.0,
                gbuffer_a: data.gbuffer_a_bindless_uav_handle.0,
                gbuffer_b: data.gbuffer_b_bindless_uav_handle.0,
                gbuffer_c: data.gbuffer_c_bindless_uav_handle.0,
                _padding0: 0, // 显式 padding 用于 uint2 对齐
                image_size: glam::uvec2(data.image_size.width, data.image_size.height).into(),
                accum_frames: data.accum_frames,
                denoise_enabled: if data.denoise_enabled { 1 } else { 0 },
                sigma_color: data.sigma_color,
                sigma_depth: data.sigma_depth,
                sigma_normal: data.sigma_normal,
                kernel_radius: data.kernel_radius,
                channel: data.channel,
                // 增强联合双边滤波参数
                sigma_albedo: data.sigma_albedo,
                sigma_position: data.sigma_position,
                scene_scale: data.scene_scale,
                // 粗糙度自适应参数
                roughness_adaptive_enabled: if data.roughness_adaptive_enabled { 1 } else { 0 },
                roughness_radius_scale: data.roughness_radius_scale,
                roughness_sigma_scale: data.roughness_sigma_scale,
            },
            glam::uvec3(
                data.image_size.width.div_ceil(gpu::denoise_accum::SHADER_X as u32),
                data.image_size.height.div_ceil(gpu::denoise_accum::SHADER_Y as u32),
                1,
            ),
        );
    }
}

/// 降噪累积 Pass 的 RenderGraph 封装
pub struct DenoiseAccumRgPass<'a> {
    pub denoise_accum_pass: &'a DenoiseAccumPass,

    pub record_ctx: RenderPassRecordCtx<'a>,

    /// 单帧 RT 输出（只读）
    pub single_frame_image: RgImageHandle,
    /// 累积结果（读写）
    pub accum_image: RgImageHandle,
    /// GBufferA：world-space forward/shading normal.xyz + roughness（只读）
    pub gbuffer_a: RgImageHandle,
    /// GBufferB：world_position.xyz + linear_depth（只读）
    pub gbuffer_b: RgImageHandle,
    /// GBufferC：albedo.rgb + metallic（只读）
    pub gbuffer_c: RgImageHandle,

    pub image_extent: vk::Extent2D,
    /// 显式传入的 pass-local 参数；不再从全局 record ctx 读取配置。
    pub settings: DenoiseAccumSettings,
    pub debug_channel: u32,
}

impl<'a> RgPass for DenoiseAccumRgPass<'a> {
    fn setup(&mut self, builder: &mut RgPassBuilder) {
        // 读取单帧 RT 输出
        builder.read_image(self.single_frame_image, RgImageState::STORAGE_READ_COMPUTE);
        // 读取 GBuffer
        builder.read_image(self.gbuffer_a, RgImageState::STORAGE_READ_COMPUTE);
        builder.read_image(self.gbuffer_b, RgImageState::STORAGE_READ_COMPUTE);
        builder.read_image(self.gbuffer_c, RgImageState::STORAGE_READ_COMPUTE);
        // 读写累积图像
        builder.read_write_image(self.accum_image, RgImageState::STORAGE_READ_WRITE_COMPUTE);
    }

    fn execute(&self, ctx: &RgPassContext) {
        let single_frame_view_handle = ctx.get_image_view_handle(self.single_frame_image).unwrap();
        let accum_view_handle = ctx.get_image_view_handle(self.accum_image).unwrap();
        let gbuffer_a_view_handle = ctx.get_image_view_handle(self.gbuffer_a).unwrap();
        let gbuffer_b_view_handle = ctx.get_image_view_handle(self.gbuffer_b).unwrap();
        let gbuffer_c_view_handle = ctx.get_image_view_handle(self.gbuffer_c).unwrap();

        let single_frame_bindless_uav_handle =
            self.record_ctx.shader_bindings.get_shader_uav_handle(single_frame_view_handle);
        let accum_bindless_uav_handle = self.record_ctx.shader_bindings.get_shader_uav_handle(accum_view_handle);
        let gbuffer_a_bindless_uav_handle =
            self.record_ctx.shader_bindings.get_shader_uav_handle(gbuffer_a_view_handle);
        let gbuffer_b_bindless_uav_handle =
            self.record_ctx.shader_bindings.get_shader_uav_handle(gbuffer_b_view_handle);
        let gbuffer_c_bindless_uav_handle =
            self.record_ctx.shader_bindings.get_shader_uav_handle(gbuffer_c_view_handle);

        self.denoise_accum_pass.exec(
            ctx.cmd,
            DenoiseAccumPassData {
                single_frame_bindless_uav_handle,
                accum_bindless_uav_handle,
                gbuffer_a_bindless_uav_handle,
                gbuffer_b_bindless_uav_handle,
                gbuffer_c_bindless_uav_handle,
                image_size: self.image_extent,
                accum_frames: self.record_ctx.view_accum.accum_frames_num() as u32,
                denoise_enabled: self.settings.enabled,
                sigma_color: self.settings.sigma_color,
                sigma_depth: self.settings.sigma_depth,
                sigma_normal: self.settings.sigma_normal,
                kernel_radius: self.settings.kernel_radius,
                channel: self.debug_channel,
                sigma_albedo: self.settings.sigma_albedo,
                sigma_position: self.settings.sigma_position,
                scene_scale: self.settings.scene_scale,
                roughness_adaptive_enabled: self.settings.roughness_adaptive_enabled,
                roughness_radius_scale: self.settings.roughness_radius_scale,
                roughness_sigma_scale: self.settings.roughness_sigma_scale,
            },
            &self.record_ctx,
        );
    }
}
