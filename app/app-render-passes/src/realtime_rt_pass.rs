use ash::vk;
use ash::vk::Handle;
use itertools::Itertools;

use truvis_descriptor_layout_macro::DescriptorBinding;
use truvis_gfx::basic::bytes::BytesConvert;
use truvis_gfx::descriptors::descriptor::GfxDescriptorSetLayout;
use truvis_gfx::resources::lifecycle::DestroyReason;
use truvis_gfx::utilities::descriptor_cursor::GfxDescriptorCursor;
use truvis_gfx::{
    commands::{barrier::GfxImageBarrier, command_buffer::GfxCommandBuffer},
    gfx::{GfxDeviceCtx, GfxDeviceInfoCtx, GfxResourceCtx},
    pipelines::shader::{GfxShaderGroupInfo, GfxShaderModuleCache, GfxShaderStageInfo},
    resources::special_buffers::sbt_buffer::GfxSBTBuffer,
};
use truvis_path::TruvisPath;
use truvis_render_foundation::handles::{GfxImageHandle, GfxImageViewHandle};
use truvis_render_foundation::render_scene_view::RenderSceneView;
use truvis_render_graph::render_graph::{RgImageHandle, RgImageState, RgPass, RgPassBuilder, RgPassContext};
use truvis_render_runtime::bindings::global_descriptor_sets::GlobalDescriptorSets;
use truvis_render_runtime::render_runtime_ctx::RenderPassRecordCtx;
use truvis_shader_binding::gpu;
use truvis_utils::count_indexed_array;
use truvis_utils::enumed_map;

pub struct GfxRtPipeline {
    pub pipeline: vk::Pipeline,
    pub pipeline_layout: vk::PipelineLayout,
}
impl Drop for GfxRtPipeline {
    fn drop(&mut self) {
        debug_assert!(self.pipeline.is_null(), "GfxRtPipeline pipeline dropped without explicit destroy");
        debug_assert!(self.pipeline_layout.is_null(), "GfxRtPipeline layout dropped without explicit destroy");
    }
}
impl GfxRtPipeline {
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
    }
}

enumed_map!(ShaderStages<GfxShaderStageInfo>: {
    RayGen: GfxShaderStageInfo {
        stage: vk::ShaderStageFlags::RAYGEN_KHR,
        entry_point: c"main_ray_gen",
        path: TruvisPath::shader_build_path_str("realtime_rt/raygen.slang"),
    },
    SkyMiss: GfxShaderStageInfo {
        stage: vk::ShaderStageFlags::MISS_KHR,
        entry_point: c"sky_miss",
        path: TruvisPath::shader_build_path_str("realtime_rt/miss_sky.slang"),
    },
    ShadowMiss: GfxShaderStageInfo {
        stage: vk::ShaderStageFlags::MISS_KHR,
        entry_point: c"shadow_miss",
        path: TruvisPath::shader_build_path_str("realtime_rt/miss_shadow.slang"),
    },
    ClosestHit: GfxShaderStageInfo {
        stage: vk::ShaderStageFlags::CLOSEST_HIT_KHR,
        entry_point: c"main_closest_hit",
        path: TruvisPath::shader_build_path_str("realtime_rt/closest_hit.slang"),
    },
    TransAny: GfxShaderStageInfo {
        stage: vk::ShaderStageFlags::ANY_HIT_KHR,
        entry_point: c"trans_any",
        path: TruvisPath::shader_build_path_str("realtime_rt/any_hit.slang"),
    },
});

enumed_map!(ShaderGroups<GfxShaderGroupInfo>: {
    RayGen: GfxShaderGroupInfo {
        ty: vk::RayTracingShaderGroupTypeKHR::GENERAL,
        general: ShaderStages::RayGen.index() as u32,
        ..GfxShaderGroupInfo::unused()
    },
    SkyMiss: GfxShaderGroupInfo {
        ty: vk::RayTracingShaderGroupTypeKHR::GENERAL,
        general: ShaderStages::SkyMiss.index() as u32,
        ..GfxShaderGroupInfo::unused()
    },
    ShadowMiss: GfxShaderGroupInfo {
        ty: vk::RayTracingShaderGroupTypeKHR::GENERAL,
        general: ShaderStages::ShadowMiss.index() as u32,
        ..GfxShaderGroupInfo::unused()
    },
    Hit: GfxShaderGroupInfo {
        ty: vk::RayTracingShaderGroupTypeKHR::TRIANGLES_HIT_GROUP,
        closest_hit: ShaderStages::ClosestHit.index() as u32,
        any_hit: ShaderStages::TransAny.index() as u32,
        ..GfxShaderGroupInfo::unused()
    },
});

/// 传入 pass 的数据
pub struct RealtimeRtPassData {
    /// 单帧 RT 输出图像
    pub single_frame_output: GfxImageHandle,
    pub single_frame_output_view: GfxImageViewHandle,
    pub single_frame_extent: vk::Extent2D,
    /// App 层 RT pipeline 设置转换后的 shader 调试通道。
    pub debug_channel: u32,
    /// HDRI / sky 直接光采样模式。
    pub sky_sampling_mode: u32,
    /// sky radiance 倍率；只缩放光照能量，不改变 importance sampling 的 PDF。
    pub sky_brightness: f32,

    // ========== GBuffer 数据 ==========
    /// GBufferA：world-space forward/shading normal.xyz + 粗糙度 roughness
    pub gbuffer_a: GfxImageHandle,
    pub gbuffer_a_view: GfxImageViewHandle,
    /// GBufferB：世界位置 world_position.xyz + 线性深度 linear_depth
    pub gbuffer_b: GfxImageHandle,
    pub gbuffer_b_view: GfxImageViewHandle,
    /// GBufferC：反照率 albedo.rgb + 金属度 metallic
    pub gbuffer_c: GfxImageHandle,
    pub gbuffer_c_view: GfxImageViewHandle,

    /// DLSS SR depth 输出。这里写的是 projection 后的 device depth，不是 GBufferB.w 的 hit distance。
    pub depth: GfxImageHandle,
    pub depth_view: GfxImageViewHandle,
    /// DLSS SR motion vectors 输出。写入 pixel-space 2D motion，包含 camera 与 object motion。
    pub motion_vectors: GfxImageHandle,
    pub motion_vectors_view: GfxImageViewHandle,
    /// DLSS RR diffuse albedo 输出。
    pub rr_diffuse_albedo: GfxImageHandle,
    pub rr_diffuse_albedo_view: GfxImageViewHandle,
    /// DLSS RR specular albedo 输出。
    pub rr_specular_albedo: GfxImageHandle,
    pub rr_specular_albedo_view: GfxImageViewHandle,
    /// DLSS RR specular motion vectors 输出。写入反射虚拟几何的 pixel-space 2D motion。
    pub rr_specular_motion_vectors: GfxImageHandle,
    pub rr_specular_motion_vectors_view: GfxImageViewHandle,
}

#[derive(DescriptorBinding)]
struct RealtimeRtDescriptorBinding {
    #[binding = 0]
    #[descriptor_type = "ACCELERATION_STRUCTURE_KHR"]
    #[stage = "RAYGEN_KHR | CLOSEST_HIT_KHR | ANY_HIT_KHR | MISS_KHR"]
    #[count = 1]
    _tlas: (),

    /// 单帧 RT 输出
    #[binding = 1]
    #[descriptor_type = "STORAGE_IMAGE"]
    #[stage = "RAYGEN_KHR | CLOSEST_HIT_KHR | ANY_HIT_KHR | MISS_KHR"]
    #[count = 1]
    _rt_single_frame_output: (),

    // ========== GBuffer 数据 ==========
    /// GBufferA：world-space forward/shading normal.xyz + 粗糙度 roughness
    #[binding = 2]
    #[descriptor_type = "STORAGE_IMAGE"]
    #[stage = "RAYGEN_KHR"]
    #[count = 1]
    _gbuffer_a: (),

    /// GBufferB：世界位置 world_position.xyz + 线性深度 linear_depth
    #[binding = 3]
    #[descriptor_type = "STORAGE_IMAGE"]
    #[stage = "RAYGEN_KHR"]
    #[count = 1]
    _gbuffer_b: (),

    /// GBufferC：反照率 albedo.rgb + 金属度 metallic
    #[binding = 4]
    #[descriptor_type = "STORAGE_IMAGE"]
    #[stage = "RAYGEN_KHR"]
    #[count = 1]
    _gbuffer_c: (),

    /// DLSS SR depth。raygen 写 storage image，后续由 Streamline 作为 kBufferTypeDepth 读取。
    #[binding = 5]
    #[descriptor_type = "STORAGE_IMAGE"]
    #[stage = "RAYGEN_KHR"]
    #[count = 1]
    _dlss_depth: (),

    /// DLSS SR motion vectors。保持 R32G32_SFLOAT，避免 Streamline 内部 mvec view 格式不匹配。
    #[binding = 6]
    #[descriptor_type = "STORAGE_IMAGE"]
    #[stage = "RAYGEN_KHR"]
    #[count = 1]
    _dlss_motion_vectors: (),

    /// DLSS RR diffuse albedo。
    #[binding = 7]
    #[descriptor_type = "STORAGE_IMAGE"]
    #[stage = "RAYGEN_KHR"]
    #[count = 1]
    _dlss_rr_diffuse_albedo: (),

    /// DLSS RR specular albedo。
    #[binding = 8]
    #[descriptor_type = "STORAGE_IMAGE"]
    #[stage = "RAYGEN_KHR"]
    #[count = 1]
    _dlss_rr_specular_albedo: (),

    /// DLSS RR specular motion vectors。
    #[binding = 9]
    #[descriptor_type = "STORAGE_IMAGE"]
    #[stage = "RAYGEN_KHR"]
    #[count = 1]
    _dlss_rr_specular_motion_vectors: (),
}

pub struct RealtimeRtPass {
    pipeline: GfxRtPipeline,
    sbt: GfxSBTBuffer,
    _rt_descriptor_set_layout: GfxDescriptorSetLayout<RealtimeRtDescriptorBinding>,
}
impl RealtimeRtPass {
    pub fn new(
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        device_info_ctx: GfxDeviceInfoCtx<'_>,
        render_descriptor_sets: &GlobalDescriptorSets,
    ) -> Self {
        let mut shader_module_cache = GfxShaderModuleCache::new();
        let stage_infos = ShaderStages::iter()
            .map(|stage| stage.value())
            .map(|stage| {
                vk::PipelineShaderStageCreateInfo::default()
                    .module(shader_module_cache.get_or_load(device_ctx, stage.path()).handle())
                    .stage(stage.stage)
                    .name(stage.entry_point)
            })
            .collect_vec();

        let shader_groups = ShaderGroups::iter()
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
            .size(size_of::<gpu::realtime_rt::PushConstants>() as u32);

        let rt_descriptor_set_layout = GfxDescriptorSetLayout::<RealtimeRtDescriptorBinding>::new(
            device_ctx,
            vk::DescriptorSetLayoutCreateFlags::PUSH_DESCRIPTOR_KHR,
            "simple-rt-descriptor-set-layout",
        );

        let pipeline_layout = {
            let mut descriptor_set_layouts = render_descriptor_sets.global_set_layouts();
            descriptor_set_layouts.push(rt_descriptor_set_layout.handle());
            let pipeline_layout_ci = vk::PipelineLayoutCreateInfo::default()
                .set_layouts(&descriptor_set_layouts)
                .push_constant_ranges(std::slice::from_ref(&push_constant_range));

            unsafe { device_ctx.device().create_pipeline_layout(&pipeline_layout_ci, None).unwrap() }
        };
        device_ctx.device().set_object_debug_name(pipeline_layout, "simple-rt-pipeline-layout");
        let pipeline_ci = vk::RayTracingPipelineCreateInfoKHR::default()
            .stages(&stage_infos)
            .groups(&shader_groups)
            .layout(pipeline_layout)
            // 这个仅仅是用来分配栈内存的，并不会在超过递归深度后让调用被丢弃
            // 需要手动跟踪递归深度
            .max_pipeline_ray_recursion_depth(2);

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
        device_ctx.device().set_object_debug_name(pipeline, "simple-rt-pipeline");

        shader_module_cache.destroy(device_ctx);

        let rt_pipeline = GfxRtPipeline {
            pipeline,
            pipeline_layout,
        };
        let sbt = GfxSBTBuffer::from_shader_groups(
            resource_ctx,
            device_ctx,
            device_info_ctx,
            rt_pipeline.pipeline,
            ShaderGroups::COUNT as u32,
            ShaderGroups::RayGen.index(),
            &[ShaderGroups::SkyMiss.index(), ShaderGroups::ShadowMiss.index()],
            &[ShaderGroups::Hit.index()],
            &[],
            "simple-rt-sbt",
        );

        Self {
            pipeline: rt_pipeline,
            sbt,
            _rt_descriptor_set_layout: rt_descriptor_set_layout,
        }
    }

    pub fn destroy(self, resource_ctx: GfxResourceCtx<'_>, device_ctx: GfxDeviceCtx<'_>) {
        let Self {
            pipeline,
            sbt,
            _rt_descriptor_set_layout,
        } = self;
        pipeline.destroy(device_ctx);
        sbt.destroy(resource_ctx, DestroyReason::Shutdown);
        _rt_descriptor_set_layout.destroy(device_ctx);
    }
    pub fn ray_trace(
        &self,
        record_ctx: &RenderPassRecordCtx<'_>,
        render_scene: &dyn RenderSceneView,
        cmd: &GfxCommandBuffer,
        pass_data: RealtimeRtPassData,
    ) {
        let frame_label = record_ctx.frame_timing.frame_label();
        let Some(tlas) = render_scene.tlas_handle(frame_label) else {
            log::trace!("RealtimeRtPass: skip ray tracing because TLAS is not ready for {}", frame_label);
            return;
        };

        let _rt_handle = record_ctx.shader_bindings.get_shader_uav_handle(pass_data.single_frame_output_view);
        let rt_image = record_ctx.gfx_resource_manager.get_image(pass_data.single_frame_output).unwrap().handle();
        let rt_image_view =
            record_ctx.gfx_resource_manager.get_image_view(pass_data.single_frame_output_view).unwrap().handle();

        // 获取 GBuffer 与 DLSS input image views。它们都由 raygen 以 storage image 写入当前 render extent。
        let gbuffer_a_view = record_ctx.gfx_resource_manager.get_image_view(pass_data.gbuffer_a_view).unwrap().handle();
        let gbuffer_b_view = record_ctx.gfx_resource_manager.get_image_view(pass_data.gbuffer_b_view).unwrap().handle();
        let gbuffer_c_view = record_ctx.gfx_resource_manager.get_image_view(pass_data.gbuffer_c_view).unwrap().handle();
        let depth_view = record_ctx.gfx_resource_manager.get_image_view(pass_data.depth_view).unwrap().handle();
        let motion_vectors_view =
            record_ctx.gfx_resource_manager.get_image_view(pass_data.motion_vectors_view).unwrap().handle();
        let rr_diffuse_albedo_view =
            record_ctx.gfx_resource_manager.get_image_view(pass_data.rr_diffuse_albedo_view).unwrap().handle();
        let rr_specular_albedo_view =
            record_ctx.gfx_resource_manager.get_image_view(pass_data.rr_specular_albedo_view).unwrap().handle();
        let rr_specular_motion_vectors_view =
            record_ctx.gfx_resource_manager.get_image_view(pass_data.rr_specular_motion_vectors_view).unwrap().handle();

        cmd.begin_label("Ray trace", glam::vec4(0.0, 1.0, 0.0, 1.0));

        cmd.cmd_bind_pipeline(vk::PipelineBindPoint::RAY_TRACING_KHR, self.pipeline.pipeline);

        cmd.push_descriptor_set(
            vk::PipelineBindPoint::RAY_TRACING_KHR,
            self.pipeline.pipeline_layout,
            gpu::REALTIME_RT_SET_NUM,
            &[
                RealtimeRtDescriptorBinding::tlas().write_tals(vk::DescriptorSet::null(), 0, vec![tlas]),
                RealtimeRtDescriptorBinding::rt_single_frame_output().write_image(
                    vk::DescriptorSet::null(),
                    0,
                    vec![
                        vk::DescriptorImageInfo::default()
                            .image_layout(vk::ImageLayout::GENERAL)
                            .image_view(rt_image_view),
                    ],
                ),
                // GBuffer 绑定
                RealtimeRtDescriptorBinding::gbuffer_a().write_image(
                    vk::DescriptorSet::null(),
                    0,
                    vec![
                        vk::DescriptorImageInfo::default()
                            .image_layout(vk::ImageLayout::GENERAL)
                            .image_view(gbuffer_a_view),
                    ],
                ),
                RealtimeRtDescriptorBinding::gbuffer_b().write_image(
                    vk::DescriptorSet::null(),
                    0,
                    vec![
                        vk::DescriptorImageInfo::default()
                            .image_layout(vk::ImageLayout::GENERAL)
                            .image_view(gbuffer_b_view),
                    ],
                ),
                RealtimeRtDescriptorBinding::gbuffer_c().write_image(
                    vk::DescriptorSet::null(),
                    0,
                    vec![
                        vk::DescriptorImageInfo::default()
                            .image_layout(vk::ImageLayout::GENERAL)
                            .image_view(gbuffer_c_view),
                    ],
                ),
                RealtimeRtDescriptorBinding::dlss_depth().write_image(
                    vk::DescriptorSet::null(),
                    0,
                    vec![
                        vk::DescriptorImageInfo::default()
                            .image_layout(vk::ImageLayout::GENERAL)
                            .image_view(depth_view),
                    ],
                ),
                RealtimeRtDescriptorBinding::dlss_motion_vectors().write_image(
                    vk::DescriptorSet::null(),
                    0,
                    vec![
                        vk::DescriptorImageInfo::default()
                            .image_layout(vk::ImageLayout::GENERAL)
                            .image_view(motion_vectors_view),
                    ],
                ),
                RealtimeRtDescriptorBinding::dlss_rr_diffuse_albedo().write_image(
                    vk::DescriptorSet::null(),
                    0,
                    vec![
                        vk::DescriptorImageInfo::default()
                            .image_layout(vk::ImageLayout::GENERAL)
                            .image_view(rr_diffuse_albedo_view),
                    ],
                ),
                RealtimeRtDescriptorBinding::dlss_rr_specular_albedo().write_image(
                    vk::DescriptorSet::null(),
                    0,
                    vec![
                        vk::DescriptorImageInfo::default()
                            .image_layout(vk::ImageLayout::GENERAL)
                            .image_view(rr_specular_albedo_view),
                    ],
                ),
                RealtimeRtDescriptorBinding::dlss_rr_specular_motion_vectors().write_image(
                    vk::DescriptorSet::null(),
                    0,
                    vec![
                        vk::DescriptorImageInfo::default()
                            .image_layout(vk::ImageLayout::GENERAL)
                            .image_view(rr_specular_motion_vectors_view),
                    ],
                ),
            ],
        );

        cmd.bind_descriptor_sets(
            vk::PipelineBindPoint::RAY_TRACING_KHR,
            self.pipeline.pipeline_layout,
            0,
            &record_ctx.shader_bindings.global_sets(frame_label),
            None,
        );
        // FIXME 这个变量废除了，现在只有 spp 1
        let spp = 1;
        let mut push_constant = gpu::realtime_rt::PushConstants {
            spp_idx: 0,
            channel: pass_data.debug_channel,
            sky_sampling_mode: pass_data.sky_sampling_mode,
            sky_brightness: pass_data.sky_brightness,
        };
        for spp_idx in 0..spp {
            push_constant.spp_idx = spp_idx;

            // 在 spp 之间，需要插入一个 image barrier，确保上一次的写入被下一次读取到
            if spp_idx != 0 {
                cmd.image_memory_barrier(
                    vk::DependencyFlags::empty(),
                    &[GfxImageBarrier::new()
                        .image(rt_image)
                        .image_aspect_flag(vk::ImageAspectFlags::COLOR)
                        .src_mask(
                            vk::PipelineStageFlags2::RAY_TRACING_SHADER_KHR,
                            vk::AccessFlags2::SHADER_WRITE | vk::AccessFlags2::SHADER_READ,
                        )
                        .dst_mask(
                            vk::PipelineStageFlags2::RAY_TRACING_SHADER_KHR,
                            vk::AccessFlags2::SHADER_READ | vk::AccessFlags2::SHADER_WRITE,
                        )],
                );
            }

            cmd.cmd_push_constants(
                self.pipeline.pipeline_layout,
                vk::ShaderStageFlags::RAYGEN_KHR
                    | vk::ShaderStageFlags::MISS_KHR
                    | vk::ShaderStageFlags::ANY_HIT_KHR
                    | vk::ShaderStageFlags::CLOSEST_HIT_KHR,
                0,
                BytesConvert::bytes_of(&push_constant),
            );

            cmd.trace_rays(
                self.sbt.raygen_region(),
                self.sbt.miss_region(),
                self.sbt.hit_region(),
                self.sbt.callable_region(),
                [
                    pass_data.single_frame_extent.width,
                    pass_data.single_frame_extent.height,
                    1,
                ],
            );
        }

        cmd.end_label();
    }
}

pub struct RealtimeRtRgPass<'a> {
    pub rt_pass: &'a RealtimeRtPass,

    pub record_ctx: RenderPassRecordCtx<'a>,
    pub render_scene: &'a dyn RenderSceneView,

    /// 单帧 RT 输出图像（只写）
    pub single_frame_image: RgImageHandle,
    pub single_frame_extent: vk::Extent2D,
    pub debug_channel: u32,
    pub sky_sampling_mode: u32,
    pub sky_brightness: f32,

    // ========== GBuffer 数据 ==========
    pub gbuffer_a: RgImageHandle,
    pub gbuffer_b: RgImageHandle,
    pub gbuffer_c: RgImageHandle,
    pub depth: RgImageHandle,
    pub motion_vectors: RgImageHandle,
    pub rr_diffuse_albedo: RgImageHandle,
    pub rr_specular_albedo: RgImageHandle,
    pub rr_specular_motion_vectors: RgImageHandle,
}
impl RgPass for RealtimeRtRgPass<'_> {
    fn setup(&mut self, builder: &mut RgPassBuilder) {
        // RT pass 写入单帧输出、GBuffer 和 DLSS SR per-pixel inputs；
        // 后续 SR/native 分支只读取这些 image，不再接传统 denoise/accum pass。
        builder.write_image(self.single_frame_image, RgImageState::STORAGE_WRITE_RAY_TRACING);
        builder.write_image(self.gbuffer_a, RgImageState::STORAGE_WRITE_RAY_TRACING);
        builder.write_image(self.gbuffer_b, RgImageState::STORAGE_WRITE_RAY_TRACING);
        builder.write_image(self.gbuffer_c, RgImageState::STORAGE_WRITE_RAY_TRACING);
        builder.write_image(self.depth, RgImageState::STORAGE_WRITE_RAY_TRACING);
        builder.write_image(self.motion_vectors, RgImageState::STORAGE_WRITE_RAY_TRACING);
        builder.write_image(self.rr_diffuse_albedo, RgImageState::STORAGE_WRITE_RAY_TRACING);
        builder.write_image(self.rr_specular_albedo, RgImageState::STORAGE_WRITE_RAY_TRACING);
        builder.write_image(self.rr_specular_motion_vectors, RgImageState::STORAGE_WRITE_RAY_TRACING);
    }

    fn execute(&self, ctx: &RgPassContext<'_>) {
        let (single_frame_image, single_frame_view) = ctx
            .get_image_and_view_handle(self.single_frame_image)
            .expect("RealtimeRtRgPass: single_frame_image not found");

        let (gbuffer_a, gbuffer_a_view) =
            ctx.get_image_and_view_handle(self.gbuffer_a).expect("RealtimeRtRgPass: gbuffer_a not found");
        let (gbuffer_b, gbuffer_b_view) =
            ctx.get_image_and_view_handle(self.gbuffer_b).expect("RealtimeRtRgPass: gbuffer_b not found");
        let (gbuffer_c, gbuffer_c_view) =
            ctx.get_image_and_view_handle(self.gbuffer_c).expect("RealtimeRtRgPass: gbuffer_c not found");
        let (depth, depth_view) = ctx.get_image_and_view_handle(self.depth).expect("RealtimeRtRgPass: depth not found");
        let (motion_vectors, motion_vectors_view) =
            ctx.get_image_and_view_handle(self.motion_vectors).expect("RealtimeRtRgPass: motion_vectors not found");
        let (rr_diffuse_albedo, rr_diffuse_albedo_view) = ctx
            .get_image_and_view_handle(self.rr_diffuse_albedo)
            .expect("RealtimeRtRgPass: rr_diffuse_albedo not found");
        let (rr_specular_albedo, rr_specular_albedo_view) = ctx
            .get_image_and_view_handle(self.rr_specular_albedo)
            .expect("RealtimeRtRgPass: rr_specular_albedo not found");
        let (rr_specular_motion_vectors, rr_specular_motion_vectors_view) = ctx
            .get_image_and_view_handle(self.rr_specular_motion_vectors)
            .expect("RealtimeRtRgPass: rr_specular_motion_vectors not found");

        self.rt_pass.ray_trace(
            &self.record_ctx,
            self.render_scene,
            ctx.cmd,
            RealtimeRtPassData {
                single_frame_output: single_frame_image,
                single_frame_output_view: single_frame_view,
                single_frame_extent: self.single_frame_extent,
                debug_channel: self.debug_channel,
                sky_sampling_mode: self.sky_sampling_mode,
                sky_brightness: self.sky_brightness,
                gbuffer_a,
                gbuffer_a_view,
                gbuffer_b,
                gbuffer_b_view,
                gbuffer_c,
                gbuffer_c_view,
                depth,
                depth_view,
                motion_vectors,
                motion_vectors_view,
                rr_diffuse_albedo,
                rr_diffuse_albedo_view,
                rr_specular_albedo,
                rr_specular_albedo_view,
                rr_specular_motion_vectors,
                rr_specular_motion_vectors_view,
            },
        );
    }
}
