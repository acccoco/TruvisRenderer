use ash::vk;
use ash::vk::Handle;
use itertools::Itertools;

use truvis_descriptor_layout_macro::DescriptorBinding;
use truvis_gfx::basic::bytes::BytesConvert;
use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_gfx::descriptors::descriptor::GfxDescriptorSetLayout;
use truvis_gfx::gfx::{GfxDeviceCtx, GfxDeviceInfoCtx, GfxResourceCtx};
use truvis_gfx::pipelines::shader::{GfxShaderGroupInfo, GfxShaderModuleCache, GfxShaderStageInfo};
use truvis_gfx::resources::lifecycle::DestroyReason;
use truvis_gfx::resources::special_buffers::sbt_buffer::GfxSBTBuffer;
use truvis_gfx::utilities::descriptor_cursor::GfxDescriptorCursor;
use truvis_path::TruvisPath;
use truvis_render_foundation::frame_timing::FrameTiming;
use truvis_render_foundation::global_descriptor_sets::GlobalDescriptorSets;
use truvis_render_foundation::shader_binding_system::ShaderBindingView;
use truvis_shader_binding::gpu;
use truvis_utils::count_indexed_array;
use truvis_utils::enumed_map;

struct RayCastRtPipeline {
    pipeline: vk::Pipeline,
    pipeline_layout: vk::PipelineLayout,
}

impl RayCastRtPipeline {
    fn destroy(mut self, ctx: GfxDeviceCtx<'_>) {
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
    }
}

impl Drop for RayCastRtPipeline {
    fn drop(&mut self) {
        debug_assert!(self.pipeline.is_null(), "RayCastRtPipeline pipeline dropped without explicit destroy");
        debug_assert!(self.pipeline_layout.is_null(), "RayCastRtPipeline layout dropped without explicit destroy");
    }
}

enumed_map!(RayCastShaderStages<GfxShaderStageInfo>: {
    RayGen: GfxShaderStageInfo {
        stage: vk::ShaderStageFlags::RAYGEN_KHR,
        entry_point: c"main_ray_gen",
        path: TruvisPath::shader_build_path_str("raycast/raycast_raygen.slang"),
    },
    Miss: GfxShaderStageInfo {
        stage: vk::ShaderStageFlags::MISS_KHR,
        entry_point: c"main_miss",
        path: TruvisPath::shader_build_path_str("raycast/raycast_miss.slang"),
    },
    ClosestHit: GfxShaderStageInfo {
        stage: vk::ShaderStageFlags::CLOSEST_HIT_KHR,
        entry_point: c"main_closest_hit",
        path: TruvisPath::shader_build_path_str("raycast/raycast_closest_hit.slang"),
    },
    AnyHit: GfxShaderStageInfo {
        stage: vk::ShaderStageFlags::ANY_HIT_KHR,
        entry_point: c"main_any_hit",
        path: TruvisPath::shader_build_path_str("raycast/raycast_any_hit.slang"),
    },
});

enumed_map!(RayCastShaderGroups<GfxShaderGroupInfo>: {
    RayGen: GfxShaderGroupInfo {
        ty: vk::RayTracingShaderGroupTypeKHR::GENERAL,
        general: RayCastShaderStages::RayGen.index() as u32,
        ..GfxShaderGroupInfo::unused()
    },
    Miss: GfxShaderGroupInfo {
        ty: vk::RayTracingShaderGroupTypeKHR::GENERAL,
        general: RayCastShaderStages::Miss.index() as u32,
        ..GfxShaderGroupInfo::unused()
    },
    Hit: GfxShaderGroupInfo {
        ty: vk::RayTracingShaderGroupTypeKHR::TRIANGLES_HIT_GROUP,
        closest_hit: RayCastShaderStages::ClosestHit.index() as u32,
        any_hit: RayCastShaderStages::AnyHit.index() as u32,
        ..GfxShaderGroupInfo::unused()
    },
});

struct RayCastSbtRegions {
    raygen: vk::StridedDeviceAddressRegionKHR,
    miss: vk::StridedDeviceAddressRegionKHR,
    hit: vk::StridedDeviceAddressRegionKHR,
    callable: vk::StridedDeviceAddressRegionKHR,
    sbt_buffer: GfxSBTBuffer,
}

impl RayCastSbtRegions {
    const RAYGEN_REGION: usize = RayCastShaderGroups::RayGen.index();
    const MISS_REGION: &'static [usize] = &[RayCastShaderGroups::Miss.index()];
    const HIT_REGION: &'static [usize] = &[RayCastShaderGroups::Hit.index()];

    fn create_sbt(
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        device_info_ctx: GfxDeviceInfoCtx<'_>,
        pipeline: &RayCastRtPipeline,
    ) -> Self {
        let rt_pipeline_props = device_info_ctx.rt_pipeline_props();
        let aligned_shader_group_handle_size = helper::align_up(
            rt_pipeline_props.shader_group_handle_size,
            rt_pipeline_props.shader_group_handle_alignment,
        );

        let raygen_size =
            helper::align_up(aligned_shader_group_handle_size, rt_pipeline_props.shader_group_base_alignment);
        let miss_size = helper::align_up(
            Self::MISS_REGION.len() as u32 * aligned_shader_group_handle_size,
            rt_pipeline_props.shader_group_base_alignment,
        );
        let hit_size = helper::align_up(
            Self::HIT_REGION.len() as u32 * aligned_shader_group_handle_size,
            rt_pipeline_props.shader_group_base_alignment,
        );
        let total_size = raygen_size + miss_size + hit_size;

        let sbt_buffer = GfxSBTBuffer::new(
            resource_ctx,
            total_size as vk::DeviceSize,
            rt_pipeline_props.shader_group_base_alignment as vk::DeviceSize,
            "raycast-sbt",
        );
        let sbt_address = sbt_buffer.device_address();

        let raygen = vk::StridedDeviceAddressRegionKHR::default()
            .stride(raygen_size as vk::DeviceSize)
            .size(raygen_size as vk::DeviceSize)
            .device_address(sbt_address);
        let miss = vk::StridedDeviceAddressRegionKHR::default()
            .stride(aligned_shader_group_handle_size as vk::DeviceSize)
            .size(miss_size as vk::DeviceSize)
            .device_address(sbt_address + raygen_size as vk::DeviceSize);
        let hit = vk::StridedDeviceAddressRegionKHR::default()
            .stride(aligned_shader_group_handle_size as vk::DeviceSize)
            .size(hit_size as vk::DeviceSize)
            .device_address(sbt_address + raygen_size as vk::DeviceSize + miss_size as vk::DeviceSize);

        let shader_group_handle_data = unsafe {
            device_ctx
                .device()
                .ray_tracing_pipeline()
                .get_ray_tracing_shader_group_handles(
                    pipeline.pipeline,
                    0,
                    RayCastShaderGroups::COUNT as u32,
                    (RayCastShaderGroups::COUNT as u32 * rt_pipeline_props.shader_group_handle_size) as usize,
                )
                .unwrap()
        };

        let copy_shader_group_handle = |group_handle_idx: usize, sbt_handle_host_addr: *mut u8| unsafe {
            let start_bytes = rt_pipeline_props.shader_group_handle_size as usize * group_handle_idx;
            let length_bytes = rt_pipeline_props.shader_group_handle_size as usize;
            let src = &shader_group_handle_data[start_bytes..start_bytes + length_bytes];
            let dst = std::slice::from_raw_parts_mut(
                sbt_handle_host_addr,
                rt_pipeline_props.shader_group_handle_size as usize,
            );
            dst.copy_from_slice(src);
        };

        let sbt_host_address = sbt_buffer.mapped_ptr();
        copy_shader_group_handle(Self::RAYGEN_REGION, sbt_host_address);

        let miss_host_address = sbt_host_address.wrapping_byte_add(raygen.size as usize);
        for (idx, group_handle_idx) in Self::MISS_REGION.iter().enumerate() {
            copy_shader_group_handle(
                *group_handle_idx,
                miss_host_address.wrapping_byte_add(idx * miss.stride as usize),
            );
        }

        let hit_host_address = miss_host_address.wrapping_byte_add(miss.size as usize);
        for (idx, group_handle_idx) in Self::HIT_REGION.iter().enumerate() {
            copy_shader_group_handle(*group_handle_idx, hit_host_address.wrapping_byte_add(idx * hit.stride as usize));
        }
        sbt_buffer.flush(resource_ctx, 0, sbt_buffer.size());

        Self {
            raygen,
            miss,
            hit,
            callable: vk::StridedDeviceAddressRegionKHR::default(),
            sbt_buffer,
        }
    }

    fn destroy(self, resource_ctx: GfxResourceCtx<'_>) {
        self.sbt_buffer.destroy(resource_ctx, DestroyReason::Shutdown);
    }
}

#[derive(DescriptorBinding)]
struct RayCastDescriptorBinding {
    #[binding = 0]
    #[descriptor_type = "ACCELERATION_STRUCTURE_KHR"]
    #[stage = "RAYGEN_KHR | CLOSEST_HIT_KHR | ANY_HIT_KHR | MISS_KHR"]
    #[count = 1]
    _tlas: (),

    #[binding = 1]
    #[descriptor_type = "STORAGE_BUFFER"]
    #[stage = "RAYGEN_KHR"]
    #[count = 1]
    _rays: (),

    #[binding = 2]
    #[descriptor_type = "STORAGE_BUFFER"]
    #[stage = "RAYGEN_KHR"]
    #[count = 1]
    _raw_hits: (),
}

pub struct RayCastPass {
    pipeline: RayCastRtPipeline,
    sbt: RayCastSbtRegions,
    descriptor_set_layout: GfxDescriptorSetLayout<RayCastDescriptorBinding>,
}

impl RayCastPass {
    pub fn new(
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        device_info_ctx: GfxDeviceInfoCtx<'_>,
        global_descriptor_sets: &GlobalDescriptorSets,
    ) -> Self {
        let mut shader_module_cache = GfxShaderModuleCache::new();
        let stage_infos = RayCastShaderStages::iter()
            .map(|stage| stage.value())
            .map(|stage| {
                vk::PipelineShaderStageCreateInfo::default()
                    .module(shader_module_cache.get_or_load(device_ctx, stage.path()).handle())
                    .stage(stage.stage)
                    .name(stage.entry_point)
            })
            .collect_vec();

        let shader_groups = RayCastShaderGroups::iter()
            .map(|group| group.value())
            .map(|group| vk::RayTracingShaderGroupCreateInfoKHR {
                ty: group.ty,
                general_shader: group.general,
                any_hit_shader: group.any_hit,
                closest_hit_shader: group.closest_hit,
                intersection_shader: group.intersection,
                ..Default::default()
            })
            .collect_vec();

        let push_constant_range = vk::PushConstantRange::default()
            .stage_flags(
                vk::ShaderStageFlags::RAYGEN_KHR
                    | vk::ShaderStageFlags::MISS_KHR
                    | vk::ShaderStageFlags::ANY_HIT_KHR
                    | vk::ShaderStageFlags::CLOSEST_HIT_KHR,
            )
            .offset(0)
            .size(size_of::<gpu::raycast::PushConstants>() as u32);

        let descriptor_set_layout = GfxDescriptorSetLayout::<RayCastDescriptorBinding>::new(
            device_ctx,
            vk::DescriptorSetLayoutCreateFlags::PUSH_DESCRIPTOR_KHR,
            "raycast-descriptor-set-layout",
        );

        let pipeline_layout = {
            let mut descriptor_set_layouts = global_descriptor_sets.global_set_layouts();
            descriptor_set_layouts.push(descriptor_set_layout.handle());
            let pipeline_layout_ci = vk::PipelineLayoutCreateInfo::default()
                .set_layouts(&descriptor_set_layouts)
                .push_constant_ranges(std::slice::from_ref(&push_constant_range));

            unsafe { device_ctx.device().create_pipeline_layout(&pipeline_layout_ci, None).unwrap() }
        };
        device_ctx.device().set_object_debug_name(pipeline_layout, "raycast-pipeline-layout");

        let pipeline_ci = vk::RayTracingPipelineCreateInfoKHR::default()
            .stages(&stage_infos)
            .groups(&shader_groups)
            .layout(pipeline_layout)
            .max_pipeline_ray_recursion_depth(1);

        let pipeline = unsafe {
            device_ctx
                .device()
                .ray_tracing_pipeline()
                .create_ray_tracing_pipelines(
                    vk::DeferredOperationKHR::null(),
                    vk::PipelineCache::null(),
                    std::slice::from_ref(&pipeline_ci),
                    None,
                )
                .unwrap()[0]
        };
        device_ctx.device().set_object_debug_name(pipeline, "raycast-pipeline");
        shader_module_cache.destroy(device_ctx);

        let pipeline = RayCastRtPipeline {
            pipeline,
            pipeline_layout,
        };
        let sbt = RayCastSbtRegions::create_sbt(resource_ctx, device_ctx, device_info_ctx, &pipeline);

        Self {
            pipeline,
            sbt,
            descriptor_set_layout,
        }
    }

    pub fn destroy(self, resource_ctx: GfxResourceCtx<'_>, device_ctx: GfxDeviceCtx<'_>) {
        let Self {
            pipeline,
            sbt,
            descriptor_set_layout,
        } = self;
        pipeline.destroy(device_ctx);
        sbt.destroy(resource_ctx);
        descriptor_set_layout.destroy(device_ctx);
    }

    pub fn trace(
        &self,
        frame_timing: &FrameTiming,
        shader_bindings: ShaderBindingView<'_>,
        tlas: vk::AccelerationStructureKHR,
        cmd: &GfxCommandBuffer,
        ray_buffer: vk::Buffer,
        raw_hit_buffer: vk::Buffer,
        ray_count: u32,
    ) {
        let frame_label = frame_timing.frame_label();
        cmd.begin_label("RayCast", glam::vec4(0.2, 0.7, 1.0, 1.0));
        cmd.cmd_bind_pipeline(vk::PipelineBindPoint::RAY_TRACING_KHR, self.pipeline.pipeline);

        cmd.push_descriptor_set(
            vk::PipelineBindPoint::RAY_TRACING_KHR,
            self.pipeline.pipeline_layout,
            gpu::RAYCAST_SET_NUM,
            &[
                RayCastDescriptorBinding::tlas().write_tals(vk::DescriptorSet::null(), 0, vec![tlas]),
                RayCastDescriptorBinding::rays().write_buffer(
                    vk::DescriptorSet::null(),
                    0,
                    vec![vk::DescriptorBufferInfo::default().buffer(ray_buffer).offset(0).range(vk::WHOLE_SIZE)],
                ),
                RayCastDescriptorBinding::raw_hits().write_buffer(
                    vk::DescriptorSet::null(),
                    0,
                    vec![vk::DescriptorBufferInfo::default().buffer(raw_hit_buffer).offset(0).range(vk::WHOLE_SIZE)],
                ),
            ],
        );

        cmd.bind_descriptor_sets(
            vk::PipelineBindPoint::RAY_TRACING_KHR,
            self.pipeline.pipeline_layout,
            0,
            &shader_bindings.global_sets(frame_label),
            None,
        );

        let push_constant = gpu::raycast::PushConstants {
            ray_count,
            _padding_0: 0,
            _padding_1: 0,
            _padding_2: 0,
        };
        cmd.cmd_push_constants(
            self.pipeline.pipeline_layout,
            vk::ShaderStageFlags::RAYGEN_KHR
                | vk::ShaderStageFlags::MISS_KHR
                | vk::ShaderStageFlags::ANY_HIT_KHR
                | vk::ShaderStageFlags::CLOSEST_HIT_KHR,
            0,
            BytesConvert::bytes_of(&push_constant),
        );

        cmd.trace_rays(&self.sbt.raygen, &self.sbt.miss, &self.sbt.hit, &self.sbt.callable, [ray_count, 1, 1]);
        cmd.end_label();
    }
}

mod helper {
    pub fn align_up(x: u32, align: u32) -> u32 {
        assert!(align.is_power_of_two());
        (x + (align - 1)) & !(align - 1)
    }
}
