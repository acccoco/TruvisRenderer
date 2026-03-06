use ash::vk;

use truvis_crate_tools::resource::TruvisPath;
use truvis_descriptor_layout_macro::DescriptorBinding;
use truvis_gfx::basic::bytes::BytesConvert;
use truvis_gfx::gfx::Gfx;
use truvis_gfx::pipelines::shader::GfxShaderModule;
use truvis_gfx::raytracing::acceleration::GfxAcceleration;
use truvis_gfx::resources::buffer::GfxBuffer;
use truvis_gfx::utilities::descriptor_cursor::GfxDescriptorCursor;
use truvis_shader_binding::truvisl;

use crate::global_descriptor_sets::GlobalDescriptorSets;
use crate::pipeline_settings::FrameLabel;

/// set 3: TLAS 绑定 + push descriptor
#[derive(DescriptorBinding)]
struct RayCastDescriptorBinding {
    #[binding = 0]
    #[descriptor_type = "ACCELERATION_STRUCTURE_KHR"]
    #[stage = "COMPUTE"]
    #[count = 1]
    _tlas: (),
}

/// push constants 布局，与 shader 对应
#[repr(C)]
struct RayCastPushConstants {
    ray_count: u32,
    _pad0: u32,
    input_buffer_addr: u64,
    output_buffer_addr: u64,
}

/// 同步射线检测器
///
/// 使用 Compute Shader + RayQuery 对场景 TLAS 进行射线追踪。
/// 独立于 RenderGraph，通过 `Gfx::one_time_exec` 同步执行，适用于拾取、碰撞检测等非渲染流水线场景。
pub struct RayCaster {
    pipeline: vk::Pipeline,
    pipeline_layout: vk::PipelineLayout,
}

impl RayCaster {
    pub fn new(global_descriptor_sets: &GlobalDescriptorSets) -> Self {
        let tlas_set_layout = {
            let binding = vk::DescriptorSetLayoutBinding::default()
                .binding(0)
                .descriptor_type(vk::DescriptorType::ACCELERATION_STRUCTURE_KHR)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::COMPUTE);

            let ci = vk::DescriptorSetLayoutCreateInfo::default()
                .flags(vk::DescriptorSetLayoutCreateFlags::PUSH_DESCRIPTOR_KHR)
                .bindings(std::slice::from_ref(&binding));

            unsafe { Gfx::get().gfx_device().create_descriptor_set_layout(&ci, None).unwrap() }
        };

        let push_constant_range = vk::PushConstantRange::default()
            .stage_flags(vk::ShaderStageFlags::COMPUTE)
            .offset(0)
            .size(size_of::<RayCastPushConstants>() as u32);

        let pipeline_layout = {
            let mut set_layouts = global_descriptor_sets.global_set_layouts();
            set_layouts.push(tlas_set_layout);

            let ci = vk::PipelineLayoutCreateInfo::default()
                .set_layouts(&set_layouts)
                .push_constant_ranges(std::slice::from_ref(&push_constant_range));

            unsafe { Gfx::get().gfx_device().create_pipeline_layout(&ci, None).unwrap() }
        };
        Gfx::get().gfx_device().set_object_debug_name(pipeline_layout, "raycast-pipeline-layout");

        // set_layout 在 pipeline_layout 创建后可以立即释放
        unsafe {
            Gfx::get().gfx_device().destroy_descriptor_set_layout(tlas_set_layout, None);
        }

        let shader_path = TruvisPath::shader_build_path_str("raycast/raycast.slang");
        let shader_module = GfxShaderModule::new(std::path::Path::new(&shader_path));

        let stage_info = vk::PipelineShaderStageCreateInfo::default()
            .module(shader_module.handle())
            .stage(vk::ShaderStageFlags::COMPUTE)
            .name(c"main_raycast");

        let pipeline_ci = vk::ComputePipelineCreateInfo::default().stage(stage_info).layout(pipeline_layout);

        let pipeline = unsafe {
            Gfx::get()
                .gfx_device()
                .create_compute_pipelines(vk::PipelineCache::null(), std::slice::from_ref(&pipeline_ci), None)
                .unwrap()[0]
        };
        Gfx::get().gfx_device().set_object_debug_name(pipeline, "raycast-pipeline");

        shader_module.destroy();

        Self {
            pipeline,
            pipeline_layout,
        }
    }

    /// 同步射线检测
    ///
    /// 向 TLAS 发射一组射线，阻塞等待 GPU 完成，返回每条射线的命中结果。
    ///
    /// # 参数
    /// - `rays`: 射线列表（起点 + 方向 + t_min/t_max）
    /// - `tlas`: 当前帧的加速结构
    /// - `global_descriptor_sets`: 全局描述符集（采样器 / bindless / per-frame + scene）
    /// - `frame_label`: 当前帧标签，用于索引正确的描述符集
    ///
    /// # 返回值
    /// 每条射线对应一个 `RayCastHit`，顺序与输入一致。`hit == 1` 表示命中。
    pub fn cast_rays(
        &self,
        rays: &[truvisl::RayCastInput],
        tlas: &GfxAcceleration,
        global_descriptor_sets: &GlobalDescriptorSets,
        frame_label: FrameLabel,
    ) -> Vec<truvisl::RayCastHit> {
        if rays.is_empty() {
            return Vec::new();
        }

        let ray_count = rays.len();
        let input_size = (ray_count * size_of::<truvisl::RayCastInput>()) as vk::DeviceSize;
        let output_size = (ray_count * size_of::<truvisl::RayCastHit>()) as vk::DeviceSize;

        // 创建 input staging buffer (CPU → GPU)
        let input_stage = GfxBuffer::new(
            input_size,
            vk::BufferUsageFlags::TRANSFER_SRC,
            None,
            true,
            "raycast-input-stage",
        );
        unsafe {
            std::ptr::copy_nonoverlapping(
                rays.as_ptr() as *const u8,
                input_stage.mapped_ptr(),
                input_size as usize,
            );
        }
        input_stage.flush(0, input_size);

        // 创建 input device buffer (GPU 端)
        let input_device = GfxBuffer::new(
            input_size,
            vk::BufferUsageFlags::STORAGE_BUFFER
                | vk::BufferUsageFlags::TRANSFER_DST
                | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
            None,
            false,
            "raycast-input-device",
        );

        // 创建 output device buffer (GPU 端)
        let output_device = GfxBuffer::new(
            output_size,
            vk::BufferUsageFlags::STORAGE_BUFFER
                | vk::BufferUsageFlags::TRANSFER_SRC
                | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
            None,
            false,
            "raycast-output-device",
        );

        // 创建 output staging buffer (GPU → CPU)
        let output_stage = GfxBuffer::new(
            output_size,
            vk::BufferUsageFlags::TRANSFER_DST,
            None,
            true,
            "raycast-output-stage",
        );

        let pipeline = self.pipeline;
        let pipeline_layout = self.pipeline_layout;
        let tlas_handle = tlas.handle();

        Gfx::get().one_time_exec(
            |cmd| {
                // 1. 上传 input 数据
                cmd.cmd_copy_buffer(
                    &input_stage,
                    &input_device,
                    &[vk::BufferCopy::default().size(input_size)],
                );

                // 2. barrier: TRANSFER → COMPUTE_SHADER
                cmd.memory_barrier(&[vk::MemoryBarrier2 {
                    src_stage_mask: vk::PipelineStageFlags2::TRANSFER,
                    dst_stage_mask: vk::PipelineStageFlags2::COMPUTE_SHADER,
                    src_access_mask: vk::AccessFlags2::TRANSFER_WRITE,
                    dst_access_mask: vk::AccessFlags2::SHADER_READ,
                    ..Default::default()
                }]);

                // 3. 绑定 pipeline
                cmd.cmd_bind_pipeline(vk::PipelineBindPoint::COMPUTE, pipeline);

                // 4. push constants
                let push = RayCastPushConstants {
                    ray_count: ray_count as u32,
                    _pad0: 0,
                    input_buffer_addr: input_device.device_address(),
                    output_buffer_addr: output_device.device_address(),
                };
                cmd.cmd_push_constants(
                    pipeline_layout,
                    vk::ShaderStageFlags::COMPUTE,
                    0,
                    BytesConvert::bytes_of(&push),
                );

                // 5. 绑定 global descriptor sets (set 0-2)
                cmd.bind_descriptor_sets(
                    vk::PipelineBindPoint::COMPUTE,
                    pipeline_layout,
                    0,
                    &global_descriptor_sets.global_sets(frame_label),
                    None,
                );

                // 6. push descriptor: TLAS (set 3)
                cmd.push_descriptor_set(
                    vk::PipelineBindPoint::COMPUTE,
                    pipeline_layout,
                    3,
                    &[RayCastDescriptorBinding::tlas().write_tals(
                        vk::DescriptorSet::null(),
                        0,
                        vec![tlas_handle],
                    )],
                );

                // 7. dispatch
                let group_count_x = (ray_count as u32 + 63) / 64;
                cmd.cmd_dispatch(glam::uvec3(group_count_x, 1, 1));

                // 8. barrier: COMPUTE_SHADER → TRANSFER
                cmd.memory_barrier(&[vk::MemoryBarrier2 {
                    src_stage_mask: vk::PipelineStageFlags2::COMPUTE_SHADER,
                    dst_stage_mask: vk::PipelineStageFlags2::TRANSFER,
                    src_access_mask: vk::AccessFlags2::SHADER_WRITE,
                    dst_access_mask: vk::AccessFlags2::TRANSFER_READ,
                    ..Default::default()
                }]);

                // 9. 拷贝结果到 staging buffer
                cmd.cmd_copy_buffer(
                    &output_device,
                    &output_stage,
                    &[vk::BufferCopy::default().size(output_size)],
                );
            },
            "raycast",
        );

        // 读取结果
        let results = unsafe {
            let ptr = output_stage.mapped_ptr() as *const truvisl::RayCastHit;
            std::slice::from_raw_parts(ptr, ray_count).to_vec()
        };

        results
    }
}

impl Drop for RayCaster {
    fn drop(&mut self) {
        let device = Gfx::get().gfx_device();
        unsafe {
            device.destroy_pipeline(self.pipeline, None);
            device.destroy_pipeline_layout(self.pipeline_layout, None);
        }
    }
}
