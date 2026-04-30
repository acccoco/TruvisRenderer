use ash::vk;

use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_path::TruvisPath;
use truvis_render_graph::compute_pass::ComputePass;
use truvis_render_graph::render_graph::{RgImageHandle, RgImageState, RgPass, RgPassBuilder, RgPassContext};
use truvis_render_interface::bindless_manager::BindlessUavHandle;
use truvis_render_interface::global_descriptor_sets::GlobalDescriptorSets;
use truvis_render_interface::render_world::RenderWorld;
use truvis_shader_binding::gpu;

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
    /// 调试通道（0 = 正常渲染，3 = 禁用累积）
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

/// 降噪累积 Pass - 对单帧 RT 结果进行双边滤波降噪，然后累积到 accum_image 中
pub struct DenoiseAccumPass {
    denoise_accum_pass: ComputePass<gpu::denoise_accum::PushConstant>,
}

impl DenoiseAccumPass {
    pub fn new(render_descriptor_sets: &GlobalDescriptorSets) -> Self {
        let denoise_accum_pass = ComputePass::<gpu::denoise_accum::PushConstant>::new(
            render_descriptor_sets,
            c"main",
            TruvisPath::shader_build_path_str("pp/denoise_accum.slang").as_str(),
        );

        Self { denoise_accum_pass }
    }

    pub fn exec(&self, cmd: &GfxCommandBuffer, data: DenoiseAccumPassData, render_world: &RenderWorld) {
        let frame_label = render_world.frame_counter.frame_label();
        self.denoise_accum_pass.exec(
            cmd,
            frame_label,
            &render_world.global_descriptor_sets,
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

    pub render_world: &'a RenderWorld,

    /// 单帧 RT 输出（只读）
    pub single_frame_image: RgImageHandle,
    /// 累积结果（读写）
    pub accum_image: RgImageHandle,
    /// GBufferA: normal.xyz + roughness（只读）
    pub gbuffer_a: RgImageHandle,
    /// GBufferB: world_position.xyz + linear_depth（只读）
    pub gbuffer_b: RgImageHandle,
    /// GBufferC: albedo.rgb + metallic（只读）
    pub gbuffer_c: RgImageHandle,

    pub image_extent: vk::Extent2D,
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
            self.render_world.bindless_manager.get_shader_uav_handle(single_frame_view_handle);
        let accum_bindless_uav_handle = self.render_world.bindless_manager.get_shader_uav_handle(accum_view_handle);
        let gbuffer_a_bindless_uav_handle =
            self.render_world.bindless_manager.get_shader_uav_handle(gbuffer_a_view_handle);
        let gbuffer_b_bindless_uav_handle =
            self.render_world.bindless_manager.get_shader_uav_handle(gbuffer_b_view_handle);
        let gbuffer_c_bindless_uav_handle =
            self.render_world.bindless_manager.get_shader_uav_handle(gbuffer_c_view_handle);

        // 从 pipeline_settings 获取降噪参数
        let denoise_settings = &self.render_world.pipeline_settings.denoise;

        self.denoise_accum_pass.exec(
            ctx.cmd,
            DenoiseAccumPassData {
                single_frame_bindless_uav_handle,
                accum_bindless_uav_handle,
                gbuffer_a_bindless_uav_handle,
                gbuffer_b_bindless_uav_handle,
                gbuffer_c_bindless_uav_handle,
                image_size: self.image_extent,
                accum_frames: self.render_world.accum_data.accum_frames_num() as u32,
                denoise_enabled: denoise_settings.enabled,
                sigma_color: denoise_settings.sigma_color,
                sigma_depth: denoise_settings.sigma_depth,
                sigma_normal: denoise_settings.sigma_normal,
                kernel_radius: denoise_settings.kernel_radius,
                channel: self.render_world.pipeline_settings.channel,
                // 增强联合双边滤波参数
                sigma_albedo: denoise_settings.sigma_albedo,
                sigma_position: denoise_settings.sigma_position,
                scene_scale: denoise_settings.scene_scale,
                // 粗糙度自适应参数
                roughness_adaptive_enabled: denoise_settings.roughness_adaptive_enabled,
                roughness_radius_scale: denoise_settings.roughness_radius_scale,
                roughness_sigma_scale: denoise_settings.roughness_sigma_scale,
            },
            self.render_world,
        );
    }
}
