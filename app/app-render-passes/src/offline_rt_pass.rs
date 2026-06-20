use ash::vk;
use itertools::Itertools;
use std::mem::size_of;

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
use truvis_utils::{count_indexed_array, enumed_map};

use crate::realtime_rt_pass::GfxRtPipeline;

// Offline RT stage 列表必须和 `engine/shader/entry/offline_rt/*` 入口保持一致。
// 这些 shader 共享 offline payload，并通过 `api/pass/offline_rt.slangi` 定义 push constant ABI。
enumed_map!(OfflineShaderStages<GfxShaderStageInfo>: {
    RayGen: GfxShaderStageInfo {
        stage: vk::ShaderStageFlags::RAYGEN_KHR,
        entry_point: c"main_ray_gen",
        path: TruvisPath::shader_build_path_str("offline_rt/raygen.slang"),
    },
    SkyMiss: GfxShaderStageInfo {
        stage: vk::ShaderStageFlags::MISS_KHR,
        entry_point: c"sky_miss",
        path: TruvisPath::shader_build_path_str("offline_rt/miss_sky.slang"),
    },
    ShadowMiss: GfxShaderStageInfo {
        stage: vk::ShaderStageFlags::MISS_KHR,
        entry_point: c"shadow_miss",
        path: TruvisPath::shader_build_path_str("offline_rt/miss_shadow.slang"),
    },
    ClosestHit: GfxShaderStageInfo {
        stage: vk::ShaderStageFlags::CLOSEST_HIT_KHR,
        entry_point: c"main_closest_hit",
        path: TruvisPath::shader_build_path_str("offline_rt/closest_hit.slang"),
    },
    TransAny: GfxShaderStageInfo {
        stage: vk::ShaderStageFlags::ANY_HIT_KHR,
        entry_point: c"trans_any",
        path: TruvisPath::shader_build_path_str("offline_rt/any_hit.slang"),
    },
});

// Shader group 顺序同时决定 SBT region 索引；新增/移动 stage 时必须同步检查
// `GfxSBTBuffer::from_shader_groups` 的 raygen/miss/hit 分组参数。
enumed_map!(OfflineShaderGroups<GfxShaderGroupInfo>: {
    RayGen: GfxShaderGroupInfo {
        ty: vk::RayTracingShaderGroupTypeKHR::GENERAL,
        general: OfflineShaderStages::RayGen.index() as u32,
        ..GfxShaderGroupInfo::unused()
    },
    SkyMiss: GfxShaderGroupInfo {
        ty: vk::RayTracingShaderGroupTypeKHR::GENERAL,
        general: OfflineShaderStages::SkyMiss.index() as u32,
        ..GfxShaderGroupInfo::unused()
    },
    ShadowMiss: GfxShaderGroupInfo {
        ty: vk::RayTracingShaderGroupTypeKHR::GENERAL,
        general: OfflineShaderStages::ShadowMiss.index() as u32,
        ..GfxShaderGroupInfo::unused()
    },
    Hit: GfxShaderGroupInfo {
        ty: vk::RayTracingShaderGroupTypeKHR::TRIANGLES_HIT_GROUP,
        closest_hit: OfflineShaderStages::ClosestHit.index() as u32,
        any_hit: OfflineShaderStages::TransAny.index() as u32,
        ..GfxShaderGroupInfo::unused()
    },
});

/// Offline RT pass 的 render-graph 执行参数。
///
/// 这里聚合的是 CPU 侧资源 handle 和离线 sample 状态；真正的 shader ABI 由
/// `gpu::offline_rt::PushConstants` 生成类型承载，避免 Rust 手写布局和 Slang drift。
pub struct OfflineRtPassData {
    pub single_frame_output: GfxImageHandle,
    pub single_frame_output_view: GfxImageViewHandle,
    pub single_frame_extent: vk::Extent2D,
    pub spp_idx: u32,
    pub sample_jitter_px: glam::Vec2,
    pub debug_channel: u32,
    pub sky_sampling_mode: u32,
    pub sky_brightness: f32,
    pub emissive_nee_enabled: bool,
    pub analytic_nee_enabled: bool,
}

/// pass-local push descriptor set。
///
/// binding 顺序必须和 `offline_rt::OFFLINE_RT_SET_NUM` 内的 Slang 声明一致：
/// 0 = TLAS，1 = 单帧 HDR storage output。全局 scene/bindless/per-frame set 仍由 GlobalDescriptorSets 提供。
#[derive(DescriptorBinding)]
struct OfflineRtDescriptorBinding {
    #[binding = 0]
    #[descriptor_type = "ACCELERATION_STRUCTURE_KHR"]
    #[stage = "RAYGEN_KHR | CLOSEST_HIT_KHR | ANY_HIT_KHR | MISS_KHR"]
    #[count = 1]
    _tlas: (),

    #[binding = 1]
    #[descriptor_type = "STORAGE_IMAGE"]
    #[stage = "RAYGEN_KHR"]
    #[count = 1]
    _rt_single_frame_output: (),
}

pub struct OfflineRtPass {
    pipeline: GfxRtPipeline,
    sbt: GfxSBTBuffer,
    _rt_descriptor_set_layout: GfxDescriptorSetLayout<OfflineRtDescriptorBinding>,
}

impl OfflineRtPass {
    pub fn new(
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        device_info_ctx: GfxDeviceInfoCtx<'_>,
        render_descriptor_sets: &GlobalDescriptorSets,
    ) -> Self {
        let mut shader_module_cache = GfxShaderModuleCache::new();
        let stage_infos = OfflineShaderStages::iter()
            .map(|stage| stage.value())
            .map(|stage| {
                vk::PipelineShaderStageCreateInfo::default()
                    .module(shader_module_cache.get_or_load(device_ctx, stage.path()).handle())
                    .stage(stage.stage)
                    .name(stage.entry_point)
            })
            .collect_vec();

        let shader_groups = OfflineShaderGroups::iter()
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

        // push constant 大小来自生成的 Slang binding；这保证新增字段或 padding 时 Rust/Slang
        // 同步失败会在生成/编译阶段暴露，而不是在运行时产生 ABI 错位。
        let push_constant_range = vk::PushConstantRange::default()
            .stage_flags(
                vk::ShaderStageFlags::RAYGEN_KHR
                    | vk::ShaderStageFlags::MISS_KHR
                    | vk::ShaderStageFlags::ANY_HIT_KHR
                    | vk::ShaderStageFlags::CLOSEST_HIT_KHR,
            )
            .offset(0)
            .size(size_of::<gpu::offline_rt::PushConstants>() as u32);

        let rt_descriptor_set_layout = GfxDescriptorSetLayout::<OfflineRtDescriptorBinding>::new(
            device_ctx,
            vk::DescriptorSetLayoutCreateFlags::PUSH_DESCRIPTOR_KHR,
            "offline-rt-descriptor-set-layout",
        );

        let pipeline_layout = {
            let mut descriptor_set_layouts = render_descriptor_sets.global_set_layouts();
            // 离线 RT 额外追加一个 push descriptor set，set number 正好等于 GLOBAL_SETS_COUNT。
            // shader 侧使用 `gpu::OFFLINE_RT_SET_NUM` 访问该集合，避免和全局 set 编号重叠。
            descriptor_set_layouts.push(rt_descriptor_set_layout.handle());
            let pipeline_layout_ci = vk::PipelineLayoutCreateInfo::default()
                .set_layouts(&descriptor_set_layouts)
                .push_constant_ranges(std::slice::from_ref(&push_constant_range));

            unsafe { device_ctx.device().create_pipeline_layout(&pipeline_layout_ci, None).unwrap() }
        };
        device_ctx.device().set_object_debug_name(pipeline_layout, "offline-rt-pipeline-layout");
        let pipeline_ci = vk::RayTracingPipelineCreateInfoKHR::default()
            .stages(&stage_infos)
            .groups(&shader_groups)
            .layout(pipeline_layout)
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
        device_ctx.device().set_object_debug_name(pipeline, "offline-rt-pipeline");

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
            OfflineShaderGroups::COUNT as u32,
            OfflineShaderGroups::RayGen.index(),
            &[
                OfflineShaderGroups::SkyMiss.index(),
                OfflineShaderGroups::ShadowMiss.index(),
            ],
            &[OfflineShaderGroups::Hit.index()],
            &[],
            "offline-rt-sbt",
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
        pass_data: OfflineRtPassData,
    ) {
        let frame_label = record_ctx.frame_timing.frame_label();
        let Some(tlas) = render_scene.tlas_handle(frame_label) else {
            // 正常路径应由 OfflinePipeline 在 graph 构建阶段提前过滤无 TLAS 帧；这里保留 warning
            // 作为防御性检查，避免 pass 被其他调用方直接执行时静默跳过。
            log::warn!("OfflineRtPass: TLAS is missing during execute for {}", frame_label);
            return;
        };

        let image = |handle: GfxImageHandle| {
            record_ctx.gfx_resource_manager.get_image(handle).expect("OfflineRtPass: image handle not found").handle()
        };
        let image_view = |handle: GfxImageViewHandle| {
            record_ctx
                .gfx_resource_manager
                .get_image_view(handle)
                .expect("OfflineRtPass: image view handle not found")
                .handle()
        };

        let output_view = image_view(pass_data.single_frame_output_view);
        let output_image = image(pass_data.single_frame_output);

        cmd.begin_label("Offline ray trace", glam::vec4(0.2, 0.8, 1.0, 1.0));
        cmd.cmd_bind_pipeline(vk::PipelineBindPoint::RAY_TRACING_KHR, self.pipeline.pipeline);

        let image_info =
            vec![vk::DescriptorImageInfo::default().image_layout(vk::ImageLayout::GENERAL).image_view(output_view)];
        // push descriptor 只绑定当前帧 TLAS 与 single-frame output，不长期持有 descriptor pool 分配。
        // 资源 layout 已由 RenderGraph 声明为 STORAGE_WRITE_RAY_TRACING。
        cmd.push_descriptor_set(
            vk::PipelineBindPoint::RAY_TRACING_KHR,
            self.pipeline.pipeline_layout,
            gpu::OFFLINE_RT_SET_NUM,
            &[
                OfflineRtDescriptorBinding::tlas().write_tals(vk::DescriptorSet::null(), 0, vec![tlas]),
                OfflineRtDescriptorBinding::rt_single_frame_output().write_image(
                    vk::DescriptorSet::null(),
                    0,
                    image_info,
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

        // 生成的 PushConstants 对应 `api/pass/offline_rt.slangi`。spp_idx 与 sample_jitter_px
        // 都来自 OfflineAccumState，使离线采样和 realtime/DLSS frame state 解耦。
        let push_constant = gpu::offline_rt::PushConstants {
            spp_idx: pass_data.spp_idx,
            channel: pass_data.debug_channel,
            sky_sampling_mode: pass_data.sky_sampling_mode,
            sky_brightness: pass_data.sky_brightness,
            emissive_nee_enabled: u32::from(pass_data.emissive_nee_enabled),
            analytic_nee_enabled: u32::from(pass_data.analytic_nee_enabled),
            sample_jitter_px: pass_data.sample_jitter_px.into(),
        };
        let shader_stages = vk::ShaderStageFlags::RAYGEN_KHR
            | vk::ShaderStageFlags::MISS_KHR
            | vk::ShaderStageFlags::ANY_HIT_KHR
            | vk::ShaderStageFlags::CLOSEST_HIT_KHR;
        cmd.cmd_push_constants(self.pipeline.pipeline_layout, shader_stages, 0, BytesConvert::bytes_of(&push_constant));
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

        // ray tracing 写完单帧 HDR 图后，后续 accum pass 会在 compute shader 中以 storage read 读取。
        // 这里补充 RT shader write -> compute storage read 的显式 image barrier。
        let image_barrier = GfxImageBarrier::new()
            .image(output_image)
            .image_aspect_flag(vk::ImageAspectFlags::COLOR)
            .src_mask(vk::PipelineStageFlags2::RAY_TRACING_SHADER_KHR, vk::AccessFlags2::SHADER_WRITE)
            .dst_mask(vk::PipelineStageFlags2::COMPUTE_SHADER, vk::AccessFlags2::SHADER_STORAGE_READ);
        cmd.image_memory_barrier(vk::DependencyFlags::empty(), std::slice::from_ref(&image_barrier));
        cmd.end_label();
    }
}

pub struct OfflineRtRgPass<'a> {
    pub rt_pass: &'a OfflineRtPass,
    pub record_ctx: RenderPassRecordCtx<'a>,
    pub render_scene: &'a dyn RenderSceneView,
    pub single_frame_image: RgImageHandle,
    pub single_frame_extent: vk::Extent2D,
    pub spp_idx: u32,
    pub sample_jitter_px: glam::Vec2,
    pub debug_channel: u32,
    pub sky_sampling_mode: u32,
    pub sky_brightness: f32,
    pub emissive_nee_enabled: bool,
    pub analytic_nee_enabled: bool,
}

impl RgPass for OfflineRtRgPass<'_> {
    fn setup(&mut self, builder: &mut RgPassBuilder) {
        builder.write_image(self.single_frame_image, RgImageState::STORAGE_WRITE_RAY_TRACING);
    }

    fn execute(&self, ctx: &RgPassContext<'_>) {
        let (single_frame_image, single_frame_view) = ctx
            .get_image_and_view_handle(self.single_frame_image)
            .expect("OfflineRtRgPass: single_frame_image not found");

        self.rt_pass.ray_trace(
            &self.record_ctx,
            self.render_scene,
            ctx.cmd,
            OfflineRtPassData {
                single_frame_output: single_frame_image,
                single_frame_output_view: single_frame_view,
                single_frame_extent: self.single_frame_extent,
                spp_idx: self.spp_idx,
                sample_jitter_px: self.sample_jitter_px,
                debug_channel: self.debug_channel,
                sky_sampling_mode: self.sky_sampling_mode,
                sky_brightness: self.sky_brightness,
                emissive_nee_enabled: self.emissive_nee_enabled,
                analytic_nee_enabled: self.analytic_nee_enabled,
            },
        );
    }
}
