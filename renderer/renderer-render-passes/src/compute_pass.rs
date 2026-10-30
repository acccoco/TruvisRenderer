use std::ffi::CStr;

use ash::vk;
use ash::vk::Handle;

use truvis_descriptor_layout_trait::DescriptorBindingLayout;
use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_gfx::descriptors::descriptor::GfxDescriptorSetLayout;
use truvis_gfx::gfx::GfxDeviceCtx;
use truvis_gfx::pipelines::shader::GfxShaderModule;
use truvis_gfx::utilities::descriptor_cursor::GfxWriteDescriptorSet;
use truvis_render_foundation::frame_label::FrameLabel;
use truvis_render_runtime::bindings::global_descriptor_sets::GlobalDescriptorSets;

/// 带单个 pass-local push descriptor set 的通用 compute pipeline。
///
/// `P` 是由 shader binding 生成的 push constant，`B` 描述当前 pass 固定角色资源的 descriptor ABI。
/// image 身份由调用方在录制 dispatch 时提供；本类型只拥有 pipeline/layout，不拥有 image 或 descriptor 内容。
pub struct ComputePass<P: bytemuck::NoUninit, B: DescriptorBindingLayout> {
    pipeline: vk::Pipeline,
    pipeline_layout: vk::PipelineLayout,
    local_descriptor_set_layout: Option<GfxDescriptorSetLayout<B>>,
    local_set_num: u32,

    _phantom: std::marker::PhantomData<(P, B)>,
}
impl<P: bytemuck::NoUninit, B: DescriptorBindingLayout> ComputePass<P, B> {
    pub fn new(
        ctx: GfxDeviceCtx<'_>,
        global_descriptor_sets: &GlobalDescriptorSets,
        local_set_num: u32,
        entry_point: &CStr,
        shader_path: &str,
    ) -> Self {
        // 输入约束先于资源创建；NoUninit 保证上传字节已初始化，ABI 仍由生成的 P 定义。
        let mut descriptor_set_layouts = global_descriptor_sets.global_set_layouts();
        assert_eq!(
            local_set_num as usize,
            descriptor_set_layouts.len(),
            "compute pass-local descriptor set must follow all global sets"
        );
        let push_size = u32::try_from(size_of::<P>()).expect("push constant size exceeds u32");
        assert!(push_size > 0 && push_size % 4 == 0, "push constant size must be nonzero and a multiple of four");

        let mut pass = Self {
            pipeline: vk::Pipeline::null(),
            pipeline_layout: vk::PipelineLayout::null(),
            local_descriptor_set_layout: None,
            local_set_num,
            _phantom: std::marker::PhantomData,
        };
        let shader_module;
        shader_module = Some(GfxShaderModule::new(ctx, std::path::Path::new(shader_path)));
        let stage_info = vk::PipelineShaderStageCreateInfo::default()
            .module(shader_module.as_ref().unwrap().handle())
            .stage(vk::ShaderStageFlags::COMPUTE)
            .name(entry_point);
        pass.local_descriptor_set_layout = Some(GfxDescriptorSetLayout::<B>::new(
            ctx,
            vk::DescriptorSetLayoutCreateFlags::PUSH_DESCRIPTOR_KHR,
            format!("{shader_path}-local-descriptor-layout"),
        ));
        descriptor_set_layouts.push(pass.local_descriptor_set_layout.as_ref().unwrap().handle());
        let push_constant_range =
            vk::PushConstantRange::default().stage_flags(vk::ShaderStageFlags::COMPUTE).offset(0).size(push_size);
        let pipeline_layout_ci = vk::PipelineLayoutCreateInfo::default()
            .set_layouts(&descriptor_set_layouts)
            .push_constant_ranges(std::slice::from_ref(&push_constant_range));
        pass.pipeline_layout = unsafe { ctx.device().create_pipeline_layout(&pipeline_layout_ci, None).unwrap() };
        let pipeline_ci = vk::ComputePipelineCreateInfo::default().stage(stage_info).layout(pass.pipeline_layout);
        pass.pipeline = match unsafe {
            ctx.device().create_compute_pipelines(vk::PipelineCache::null(), std::slice::from_ref(&pipeline_ci), None)
        } {
            Ok(pipelines) => pipelines[0],
            Err((pipelines, error)) => {
                for pipeline in pipelines.into_iter().filter(|pipeline| !pipeline.is_null()) {
                    unsafe {
                        ctx.device().destroy_pipeline(pipeline, None);
                    }
                }
                panic!("failed to create compute pipeline: {error:?}");
            }
        };
        if let Some(shader_module) = shader_module {
            shader_module.destroy(ctx);
        }

        pass
    }

    pub fn exec(
        &self,
        cmd: &GfxCommandBuffer,
        frame_label: FrameLabel,
        global_descriptor_sets: &GlobalDescriptorSets,
        descriptor_writes: &[GfxWriteDescriptorSet],
        params: &P,
        group_cnt: glam::UVec3,
    ) {
        cmd.cmd_bind_pipeline(vk::PipelineBindPoint::COMPUTE, self.pipeline);

        cmd.push_descriptor_set(
            vk::PipelineBindPoint::COMPUTE,
            self.pipeline_layout,
            self.local_set_num,
            descriptor_writes,
        );

        cmd.cmd_push_constants(self.pipeline_layout, vk::ShaderStageFlags::COMPUTE, 0, bytemuck::bytes_of(params));
        cmd.bind_descriptor_sets(
            vk::PipelineBindPoint::COMPUTE,
            self.pipeline_layout,
            0,
            &global_descriptor_sets.global_sets(frame_label),
            None,
        );

        cmd.cmd_dispatch(group_cnt);
    }

    pub fn destroy(mut self, ctx: GfxDeviceCtx<'_>) {
        if !self.pipeline.is_null() {
            unsafe {
                ctx.device().destroy_pipeline(self.pipeline, None);
            }
            self.pipeline = vk::Pipeline::null();
        }
        if !self.pipeline_layout.is_null() {
            unsafe {
                ctx.device().destroy_pipeline_layout(self.pipeline_layout, None);
            }
            self.pipeline_layout = vk::PipelineLayout::null();
        }
        if let Some(layout) = self.local_descriptor_set_layout.take() {
            layout.destroy(ctx);
        }
    }
}
impl<P: bytemuck::NoUninit, B: DescriptorBindingLayout> Drop for ComputePass<P, B> {
    fn drop(&mut self) {
        debug_assert!(self.pipeline.is_null(), "ComputePass pipeline dropped without explicit destroy");
        debug_assert!(self.pipeline_layout.is_null(), "ComputePass layout dropped without explicit destroy");
        debug_assert!(
            self.local_descriptor_set_layout.is_none(),
            "ComputePass descriptor layout dropped without explicit destroy"
        );
    }
}
