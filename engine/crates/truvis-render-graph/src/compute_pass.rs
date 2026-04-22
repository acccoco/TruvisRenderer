use std::ffi::CStr;

use ash::vk;

use truvis_gfx::basic::bytes::BytesConvert;
use truvis_gfx::{commands::command_buffer::GfxCommandBuffer, gfx::Gfx, pipelines::shader::GfxShaderModule};
use truvis_render_interface::global_descriptor_sets::GlobalDescriptorSets;
use truvis_render_interface::pipeline_settings::FrameLabel;

/// 泛型参数 P 表示 compute shader 的参数，以 push constant 的形式传入 shader
pub struct ComputePass<P: Sized> {
    pipeline: vk::Pipeline,
    pipeline_layout: vk::PipelineLayout,

    _phantom: std::marker::PhantomData<P>,
}
impl<P: Sized> ComputePass<P> {
    pub fn new(global_descriptor_sets: &GlobalDescriptorSets, entry_point: &CStr, shader_path: &str) -> Self {
        let shader_module = GfxShaderModule::new(std::path::Path::new(shader_path));
        let stage_info = vk::PipelineShaderStageCreateInfo::default()
            .module(shader_module.handle())
            .stage(vk::ShaderStageFlags::COMPUTE)
            .name(entry_point);

        let pipeline_layout = {
            let push_constant_range = vk::PushConstantRange::default()
                .stage_flags(vk::ShaderStageFlags::COMPUTE)
                .offset(0)
                .size(size_of::<P>() as u32);

            let descriptor_set_layouts = global_descriptor_sets.global_set_layouts();
            let pipeline_layout_ci = vk::PipelineLayoutCreateInfo::default()
                .set_layouts(&descriptor_set_layouts)
                .push_constant_ranges(std::slice::from_ref(&push_constant_range));

            unsafe { Gfx::get().gfx_device().create_pipeline_layout(&pipeline_layout_ci, None).unwrap() }
        };

        let pipeline_ci = vk::ComputePipelineCreateInfo::default().stage(stage_info).layout(pipeline_layout);
        let pipeline = unsafe {
            Gfx::get()
                .gfx_device()
                .create_compute_pipelines(vk::PipelineCache::null(), std::slice::from_ref(&pipeline_ci), None)
                .unwrap()[0]
        };

        shader_module.destroy();

        Self {
            pipeline,
            pipeline_layout,

            _phantom: std::marker::PhantomData,
        }
    }

    pub fn exec(
        &self,
        cmd: &GfxCommandBuffer,
        frame_label: FrameLabel,
        global_descriptor_sets: &GlobalDescriptorSets,
        params: &P,
        group_cnt: glam::UVec3,
    ) {
        cmd.cmd_bind_pipeline(vk::PipelineBindPoint::COMPUTE, self.pipeline);

        cmd.cmd_push_constants(self.pipeline_layout, vk::ShaderStageFlags::COMPUTE, 0, BytesConvert::bytes_of(params));
        cmd.bind_descriptor_sets(
            vk::PipelineBindPoint::COMPUTE,
            self.pipeline_layout,
            0,
            &global_descriptor_sets.global_sets(frame_label),
            None,
        );

        cmd.cmd_dispatch(group_cnt);
    }

    pub fn destroy(self) {
        // drop
    }
}
impl<P: Sized> Drop for ComputePass<P> {
    fn drop(&mut self) {
        let gfx_device = Gfx::get().gfx_device();
        unsafe {
            gfx_device.destroy_pipeline(self.pipeline, None);
            gfx_device.destroy_pipeline_layout(self.pipeline_layout, None);
        }
    }
}
