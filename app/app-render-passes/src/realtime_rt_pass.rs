use ash::vk;
use ash::vk::Handle;
use enum_map::{Enum, EnumMap, enum_map};
use itertools::Itertools;
use std::sync::LazyLock;

use std::rc::Rc;

use truvis_descriptor_layout_macro::DescriptorBinding;
use truvis_gfx::basic::bytes::BytesConvert;
use truvis_gfx::descriptors::descriptor::{GfxDescriptorSet, GfxDescriptorSetLayout};
use truvis_gfx::descriptors::descriptor_pool::{GfxDescriptorPool, GfxDescriptorPoolCreateInfo};
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

// Shader stage 的 enum 声明顺序就是 VkPipelineShaderStageCreateInfo 数组顺序；
// shader group 通过这个顺序写入 stage index，新增或移动变体时必须同步检查下方 group 表。
#[derive(Debug, Clone, Copy, Enum)]
enum ShaderStages {
    RayGen,
    SkyMiss,
    ShadowMiss,
    ClosestHit,
    TransAny,
}

impl ShaderStages {
    fn index(self) -> usize {
        self.into_usize()
    }
}

static SHADER_STAGES: LazyLock<EnumMap<ShaderStages, GfxShaderStageInfo>> = LazyLock::new(|| {
    enum_map! {
        ShaderStages::RayGen => GfxShaderStageInfo {
            stage: vk::ShaderStageFlags::RAYGEN_KHR,
            entry_point: c"main_ray_gen",
            path: TruvisPath::shader_build_path_str("realtime_rt/raygen.slang"),
        },
        ShaderStages::SkyMiss => GfxShaderStageInfo {
            stage: vk::ShaderStageFlags::MISS_KHR,
            entry_point: c"sky_miss",
            path: TruvisPath::shader_build_path_str("realtime_rt/miss_sky.slang"),
        },
        ShaderStages::ShadowMiss => GfxShaderStageInfo {
            stage: vk::ShaderStageFlags::MISS_KHR,
            entry_point: c"shadow_miss",
            path: TruvisPath::shader_build_path_str("realtime_rt/miss_shadow.slang"),
        },
        ShaderStages::ClosestHit => GfxShaderStageInfo {
            stage: vk::ShaderStageFlags::CLOSEST_HIT_KHR,
            entry_point: c"main_closest_hit",
            path: TruvisPath::shader_build_path_str("realtime_rt/closest_hit.slang"),
        },
        ShaderStages::TransAny => GfxShaderStageInfo {
            stage: vk::ShaderStageFlags::ANY_HIT_KHR,
            entry_point: c"trans_any",
            path: TruvisPath::shader_build_path_str("realtime_rt/any_hit.slang"),
        },
    }
});

// Shader group 的 enum 声明顺序同时决定 SBT region 索引；
// `GfxSBTBuffer::from_shader_groups` 的 raygen/miss/hit 参数必须和这里保持一致。
#[derive(Debug, Clone, Copy, Enum)]
enum ShaderGroups {
    RayGen,
    SkyMiss,
    ShadowMiss,
    Hit,
}

impl ShaderGroups {
    const COUNT: usize = <Self as Enum>::LENGTH;

    fn index(self) -> usize {
        self.into_usize()
    }
}

static SHADER_GROUPS: LazyLock<EnumMap<ShaderGroups, GfxShaderGroupInfo>> = LazyLock::new(|| {
    enum_map! {
        ShaderGroups::RayGen => GfxShaderGroupInfo {
            ty: vk::RayTracingShaderGroupTypeKHR::GENERAL,
            general: ShaderStages::RayGen.index() as u32,
            ..GfxShaderGroupInfo::unused()
        },
        ShaderGroups::SkyMiss => GfxShaderGroupInfo {
            ty: vk::RayTracingShaderGroupTypeKHR::GENERAL,
            general: ShaderStages::SkyMiss.index() as u32,
            ..GfxShaderGroupInfo::unused()
        },
        ShaderGroups::ShadowMiss => GfxShaderGroupInfo {
            ty: vk::RayTracingShaderGroupTypeKHR::GENERAL,
            general: ShaderStages::ShadowMiss.index() as u32,
            ..GfxShaderGroupInfo::unused()
        },
        ShaderGroups::Hit => GfxShaderGroupInfo {
            ty: vk::RayTracingShaderGroupTypeKHR::TRIANGLES_HIT_GROUP,
            closest_hit: ShaderStages::ClosestHit.index() as u32,
            any_hit: ShaderStages::TransAny.index() as u32,
            ..GfxShaderGroupInfo::unused()
        },
    }
});

/// RenderGraph 中的 ReSTIR DI reservoir image handles。
#[derive(Clone, Copy)]
pub struct RestirReservoirRgImages {
    pub a: RgImageHandle,
    pub b: RgImageHandle,
    pub c: RgImageHandle,
    pub d: RgImageHandle,
}

/// RenderGraph 中的 ReSTIR DI primary surface key image handles。
#[derive(Clone, Copy)]
pub struct RestirSurfaceKeyRgImages {
    pub a: RgImageHandle,
    pub b: RgImageHandle,
    pub c: RgImageHandle,
}

/// ReSTIR DI reservoir pass image handles。
///
/// app-kit 负责资源生命周期；render-pass crate 只关心当前 descriptor 绑定需要的
/// Vulkan image/view handle，避免把 app 层 target owner 类型反向暴露到 pass 实现里。
#[derive(Clone, Copy)]
pub struct RestirReservoirPassImages {
    pub a: GfxImageHandle,
    pub a_view: GfxImageViewHandle,
    pub b: GfxImageHandle,
    pub b_view: GfxImageViewHandle,
    pub c: GfxImageHandle,
    pub c_view: GfxImageViewHandle,
    pub d: GfxImageHandle,
    pub d_view: GfxImageViewHandle,
}

/// ReSTIR DI primary surface key pass image handles。
///
/// 这三张图像在 pass 内按 current/history 分别绑定。它们保存 ReSTIR 自己的 primary
/// surface 签名，不能被 DLSS/RR GBuffer 替代。
#[derive(Clone, Copy)]
pub struct RestirSurfaceKeyPassImages {
    pub a: GfxImageHandle,
    pub a_view: GfxImageViewHandle,
    pub b: GfxImageHandle,
    pub b_view: GfxImageViewHandle,
    pub c: GfxImageHandle,
    pub c_view: GfxImageViewHandle,
}

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
    /// 是否启用自发光三角形 NEE。
    pub emissive_nee_enabled: bool,
    /// 是否启用 analytic light NEE。
    pub analytic_nee_enabled: bool,
    /// Primary ReSTIR DI shader 模式。
    pub restir_di_mode: u32,
    /// 当前 previous frame label 的 temporal reservoir / surface key 是否可参与 temporal reuse。
    /// 该标志只表达 CPU 侧 frame/reset/mode 连续性；scene light 版本仍由 shader metadata 拒绝。
    pub restir_history_valid: bool,

    // ========== SHARC world-space radiance cache ==========
    // buffer 句柄不在这里传：它们是持久资源，已在 pass 创建时写入 SHARC regular descriptor set。
    // 这里只传每帧可调的 push constant 参数。
    /// SHARC 模式（0=Off / 1=Update / 2=On）。Off 时不发 Update/Resolve sub-pass。
    pub sharc_mode: u32,
    /// SHARC hash map 容量（voxel 数），同时决定 Resolve dispatch 规模。
    pub sharc_capacity: u32,
    /// SHARC scene scale，控制 voxel 物理尺寸。
    pub sharc_scene_scale: f32,

    // ========== ReSTIR DI 数据 ==========
    pub restir_initial: RestirReservoirPassImages,
    pub restir_temporal: RestirReservoirPassImages,
    pub restir_final: RestirReservoirPassImages,
    pub restir_history: RestirReservoirPassImages,
    pub restir_surface: RestirSurfaceKeyPassImages,
    pub restir_history_surface: RestirSurfaceKeyPassImages,

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

    /// ReSTIR DI initial reservoir。
    #[binding = 10]
    #[descriptor_type = "STORAGE_IMAGE"]
    #[stage = "RAYGEN_KHR"]
    #[count = 1]
    _restir_initial_a: (),
    #[binding = 11]
    #[descriptor_type = "STORAGE_IMAGE"]
    #[stage = "RAYGEN_KHR"]
    #[count = 1]
    _restir_initial_b: (),
    #[binding = 12]
    #[descriptor_type = "STORAGE_IMAGE"]
    #[stage = "RAYGEN_KHR"]
    #[count = 1]
    _restir_initial_c: (),
    #[binding = 13]
    #[descriptor_type = "STORAGE_IMAGE"]
    #[stage = "RAYGEN_KHR"]
    #[count = 1]
    _restir_initial_d: (),

    /// ReSTIR DI temporal reservoir。
    #[binding = 14]
    #[descriptor_type = "STORAGE_IMAGE"]
    #[stage = "RAYGEN_KHR"]
    #[count = 1]
    _restir_temporal_a: (),
    #[binding = 15]
    #[descriptor_type = "STORAGE_IMAGE"]
    #[stage = "RAYGEN_KHR"]
    #[count = 1]
    _restir_temporal_b: (),
    #[binding = 16]
    #[descriptor_type = "STORAGE_IMAGE"]
    #[stage = "RAYGEN_KHR"]
    #[count = 1]
    _restir_temporal_c: (),
    #[binding = 17]
    #[descriptor_type = "STORAGE_IMAGE"]
    #[stage = "RAYGEN_KHR"]
    #[count = 1]
    _restir_temporal_d: (),

    /// ReSTIR DI final reservoir。它只服务当前帧 spatial/final shade，不作为下一帧 temporal history。
    #[binding = 18]
    #[descriptor_type = "STORAGE_IMAGE"]
    #[stage = "RAYGEN_KHR"]
    #[count = 1]
    _restir_final_a: (),
    #[binding = 19]
    #[descriptor_type = "STORAGE_IMAGE"]
    #[stage = "RAYGEN_KHR"]
    #[count = 1]
    _restir_final_b: (),
    #[binding = 20]
    #[descriptor_type = "STORAGE_IMAGE"]
    #[stage = "RAYGEN_KHR"]
    #[count = 1]
    _restir_final_c: (),
    #[binding = 21]
    #[descriptor_type = "STORAGE_IMAGE"]
    #[stage = "RAYGEN_KHR"]
    #[count = 1]
    _restir_final_d: (),

    /// Previous frame ReSTIR DI temporal history reservoir。
    ///
    /// 绑定顺序必须与 shader API 保持一致；Rust 侧故意绑定上一帧 temporal reservoir，
    /// 避免 spatial final 的邻域样本跨帧反馈到 temporal reuse。
    #[binding = 22]
    #[descriptor_type = "STORAGE_IMAGE"]
    #[stage = "RAYGEN_KHR"]
    #[count = 1]
    _restir_history_a: (),
    #[binding = 23]
    #[descriptor_type = "STORAGE_IMAGE"]
    #[stage = "RAYGEN_KHR"]
    #[count = 1]
    _restir_history_b: (),
    #[binding = 24]
    #[descriptor_type = "STORAGE_IMAGE"]
    #[stage = "RAYGEN_KHR"]
    #[count = 1]
    _restir_history_c: (),
    #[binding = 25]
    #[descriptor_type = "STORAGE_IMAGE"]
    #[stage = "RAYGEN_KHR"]
    #[count = 1]
    _restir_history_d: (),

    /// Current / previous primary surface key。
    ///
    /// current key 由 path phase 写入并被 temporal/spatial/final 读取；previous key
    /// 只参与 temporal surface rejection，不参与 DLSS/RR history。
    #[binding = 26]
    #[descriptor_type = "STORAGE_IMAGE"]
    #[stage = "RAYGEN_KHR"]
    #[count = 1]
    _restir_surface_a: (),
    #[binding = 27]
    #[descriptor_type = "STORAGE_IMAGE"]
    #[stage = "RAYGEN_KHR"]
    #[count = 1]
    _restir_surface_b: (),
    #[binding = 28]
    #[descriptor_type = "STORAGE_IMAGE"]
    #[stage = "RAYGEN_KHR"]
    #[count = 1]
    _restir_surface_c: (),
    #[binding = 29]
    #[descriptor_type = "STORAGE_IMAGE"]
    #[stage = "RAYGEN_KHR"]
    #[count = 1]
    _restir_history_surface_a: (),
    #[binding = 30]
    #[descriptor_type = "STORAGE_IMAGE"]
    #[stage = "RAYGEN_KHR"]
    #[count = 1]
    _restir_history_surface_b: (),
    #[binding = 31]
    #[descriptor_type = "STORAGE_IMAGE"]
    #[stage = "RAYGEN_KHR"]
    #[count = 1]
    _restir_history_surface_c: (),
}

/// SHARC 世界空间缓存 buffer 的独立 push descriptor set。
///
/// 单独成 set 而不是塞进 `RealtimeRtDescriptorBinding`：后者已有 32 个 binding，正好等于多数
/// 设备的 `maxPushDescriptors`，再加会超限。SHARC 三个 buffer 即使 Off 也绑定（raygen 静态引用）。
#[derive(DescriptorBinding)]
struct SharcDescriptorBinding {
    /// hash entry buffer（uint64 hash key）。
    #[binding = 0]
    #[descriptor_type = "STORAGE_BUFFER"]
    #[stage = "RAYGEN_KHR"]
    #[count = 1]
    _sharc_hash_entries: (),
    /// accumulation buffer（本帧原子累积）。
    #[binding = 1]
    #[descriptor_type = "STORAGE_BUFFER"]
    #[stage = "RAYGEN_KHR"]
    #[count = 1]
    _sharc_accumulation: (),
    /// resolved buffer（跨帧累积结果）。
    #[binding = 2]
    #[descriptor_type = "STORAGE_BUFFER"]
    #[stage = "RAYGEN_KHR"]
    #[count = 1]
    _sharc_resolved: (),
}

pub struct RealtimeRtPass {
    pipeline: GfxRtPipeline,
    sbt: GfxSBTBuffer,
    _rt_descriptor_set_layout: GfxDescriptorSetLayout<RealtimeRtDescriptorBinding>,
    /// SHARC buffer 的独立 **regular**（非 push）descriptor set layout（set = REALTIME_RT_SET_NUM + 1）。
    ///
    /// 一个 pipeline layout 只允许一个 push descriptor set（realtime set 已占用），因此 SHARC 走普通
    /// allocated set。SHARC buffer 是持久资源（不随帧/resize 变化），所以这个 set 在 pass 创建时分配并
    /// 一次性写入，之后每帧只 bind，不再更新。
    sharc_descriptor_set_layout: GfxDescriptorSetLayout<SharcDescriptorBinding>,
    sharc_descriptor_pool: GfxDescriptorPool,
    sharc_descriptor_set: GfxDescriptorSet<SharcDescriptorBinding>,
}
impl RealtimeRtPass {
    pub fn new(
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        device_info_ctx: GfxDeviceInfoCtx<'_>,
        render_descriptor_sets: &GlobalDescriptorSets,
        sharc_hash_entries: vk::Buffer,
        sharc_accumulation: vk::Buffer,
        sharc_resolved: vk::Buffer,
    ) -> Self {
        let mut shader_module_cache = GfxShaderModuleCache::new();
        let stage_infos = SHADER_STAGES
            .values()
            .map(|stage| {
                vk::PipelineShaderStageCreateInfo::default()
                    .module(shader_module_cache.get_or_load(device_ctx, stage.path()).handle())
                    .stage(stage.stage)
                    .name(stage.entry_point)
            })
            .collect_vec();

        let shader_groups = SHADER_GROUPS
            .values()
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
        // SHARC buffer 独立成 set：避免 realtime push set 超过 maxPushDescriptors，也避免一个 pipeline layout
        // 出现两个 push set（Vulkan 不允许）。因此这里用 **regular** set（无 PUSH_DESCRIPTOR flag）。
        let sharc_descriptor_set_layout = GfxDescriptorSetLayout::<SharcDescriptorBinding>::new(
            device_ctx,
            vk::DescriptorSetLayoutCreateFlags::empty(),
            "sharc-descriptor-set-layout",
        );

        let pipeline_layout = {
            // set 顺序必须与 shader 一致：global sets..., REALTIME_RT_SET_NUM(push), SHARC_SET_NUM(regular)。
            let mut descriptor_set_layouts = render_descriptor_sets.global_set_layouts();
            descriptor_set_layouts.push(rt_descriptor_set_layout.handle());
            descriptor_set_layouts.push(sharc_descriptor_set_layout.handle());
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

        // SHARC regular descriptor set：分配一次并写入持久 buffer，之后每帧只 bind 不再更新。
        // SHARC buffer 由 app 层 SharcTargets 拥有，不随帧/resize 变化，所以这种「一次写入」是安全的。
        let sharc_descriptor_pool = GfxDescriptorPool::new(
            device_ctx,
            Rc::new(GfxDescriptorPoolCreateInfo::new(
                vk::DescriptorPoolCreateFlags::empty(),
                1,
                vec![vk::DescriptorPoolSize {
                    ty: vk::DescriptorType::STORAGE_BUFFER,
                    descriptor_count: 3,
                }],
            )),
            "sharc-descriptor-pool",
        );
        let sharc_descriptor_set = GfxDescriptorSet::<SharcDescriptorBinding>::new(
            device_ctx,
            &sharc_descriptor_pool,
            &sharc_descriptor_set_layout,
            "sharc-descriptor-set",
        );
        let sharc_buffer_info = |buffer: vk::Buffer| {
            vec![vk::DescriptorBufferInfo::default().buffer(buffer).offset(0).range(vk::WHOLE_SIZE)]
        };
        device_ctx.device().write_descriptor_sets(&[
            SharcDescriptorBinding::sharc_hash_entries().write_buffer(
                sharc_descriptor_set.handle(),
                0,
                sharc_buffer_info(sharc_hash_entries),
            ),
            SharcDescriptorBinding::sharc_accumulation().write_buffer(
                sharc_descriptor_set.handle(),
                0,
                sharc_buffer_info(sharc_accumulation),
            ),
            SharcDescriptorBinding::sharc_resolved().write_buffer(
                sharc_descriptor_set.handle(),
                0,
                sharc_buffer_info(sharc_resolved),
            ),
        ]);

        Self {
            pipeline: rt_pipeline,
            sbt,
            _rt_descriptor_set_layout: rt_descriptor_set_layout,
            sharc_descriptor_set_layout,
            sharc_descriptor_pool,
            sharc_descriptor_set,
        }
    }

    pub fn destroy(self, resource_ctx: GfxResourceCtx<'_>, device_ctx: GfxDeviceCtx<'_>) {
        let Self {
            pipeline,
            sbt,
            _rt_descriptor_set_layout,
            sharc_descriptor_set_layout,
            sharc_descriptor_pool,
            sharc_descriptor_set: _sharc_descriptor_set,
        } = self;
        pipeline.destroy(device_ctx);
        sbt.destroy(resource_ctx, DestroyReason::Shutdown);
        _rt_descriptor_set_layout.destroy(device_ctx);
        // descriptor set 跟随 pool 释放；显式释放 pool 与 layout。
        sharc_descriptor_pool.destroy(device_ctx);
        sharc_descriptor_set_layout.destroy(device_ctx);
    }
    pub fn ray_trace(
        &self,
        record_ctx: &RenderPassRecordCtx<'_>,
        render_scene: &dyn RenderSceneView,
        cmd: &GfxCommandBuffer,
        pass_data: RealtimeRtPassData,
    ) {
        const RESTIR_DI_MODE_OFF: u32 = 0;
        const RESTIR_DI_MODE_INITIAL_ONLY: u32 = 1;
        const RESTIR_DI_MODE_TEMPORAL_SPATIAL: u32 = 3;
        const RESTIR_DI_PHASE_PATH: u32 = 0;
        const RESTIR_DI_PHASE_TEMPORAL: u32 = 1;
        const RESTIR_DI_PHASE_SPATIAL: u32 = 2;
        const RESTIR_DI_PHASE_FINAL: u32 = 3;

        // SHARC sub-pass 常量，必须与 `api/pass/realtime_rt.slangi` 的 SHARC_PHASE_* 保持一致。
        const SHARC_MODE_OFF: u32 = 0;
        const SHARC_PHASE_NONE: u32 = 0;
        const SHARC_PHASE_UPDATE: u32 = 1;
        const SHARC_PHASE_RESOLVE: u32 = 2;
        // Resolve dispatch 的 2D 网格宽度；entry index = y * width + x。height 按 capacity 上取整。
        const SHARC_RESOLVE_DISPATCH_WIDTH: u32 = 2048;

        let frame_label = record_ctx.frame_timing.frame_label();
        let Some(tlas) = render_scene.tlas_handle(frame_label) else {
            log::trace!("RealtimeRtPass: skip ray tracing because TLAS is not ready for {}", frame_label);
            return;
        };

        let image = |handle: GfxImageHandle| {
            record_ctx.gfx_resource_manager.get_image(handle).expect("RealtimeRtPass: image handle not found").handle()
        };
        let image_view = |handle: GfxImageViewHandle| {
            record_ctx
                .gfx_resource_manager
                .get_image_view(handle)
                .expect("RealtimeRtPass: image view handle not found")
                .handle()
        };

        let _rt_handle = record_ctx.shader_bindings.get_shader_uav_handle(pass_data.single_frame_output_view);
        let rt_image_view = image_view(pass_data.single_frame_output_view);

        // 获取 GBuffer 与 DLSS input image views。它们都由 raygen 以 storage image 写入当前 render extent。
        let gbuffer_a_view = image_view(pass_data.gbuffer_a_view);
        let gbuffer_b_view = image_view(pass_data.gbuffer_b_view);
        let gbuffer_c_view = image_view(pass_data.gbuffer_c_view);
        let depth_view = image_view(pass_data.depth_view);
        let motion_vectors_view = image_view(pass_data.motion_vectors_view);
        let rr_diffuse_albedo_view = image_view(pass_data.rr_diffuse_albedo_view);
        let rr_specular_albedo_view = image_view(pass_data.rr_specular_albedo_view);
        let rr_specular_motion_vectors_view = image_view(pass_data.rr_specular_motion_vectors_view);

        // ReSTIR DI 资源通过 pass-local push descriptor 绑定，顺序必须与
        // `api/pass/realtime_rt.slangi` 完全一致。这里显式展开 A/B/C/D，避免把
        // reservoir pack 的 uint/float 图像顺序藏进动态数组导致 ABI 难以审查。
        let restir_initial_a_view = image_view(pass_data.restir_initial.a_view);
        let restir_initial_b_view = image_view(pass_data.restir_initial.b_view);
        let restir_initial_c_view = image_view(pass_data.restir_initial.c_view);
        let restir_initial_d_view = image_view(pass_data.restir_initial.d_view);
        let restir_temporal_a_view = image_view(pass_data.restir_temporal.a_view);
        let restir_temporal_b_view = image_view(pass_data.restir_temporal.b_view);
        let restir_temporal_c_view = image_view(pass_data.restir_temporal.c_view);
        let restir_temporal_d_view = image_view(pass_data.restir_temporal.d_view);
        let restir_final_a_view = image_view(pass_data.restir_final.a_view);
        let restir_final_b_view = image_view(pass_data.restir_final.b_view);
        let restir_final_c_view = image_view(pass_data.restir_final.c_view);
        let restir_final_d_view = image_view(pass_data.restir_final.d_view);
        let restir_history_a_view = image_view(pass_data.restir_history.a_view);
        let restir_history_b_view = image_view(pass_data.restir_history.b_view);
        let restir_history_c_view = image_view(pass_data.restir_history.c_view);
        let restir_history_d_view = image_view(pass_data.restir_history.d_view);
        let restir_surface_a_view = image_view(pass_data.restir_surface.a_view);
        let restir_surface_b_view = image_view(pass_data.restir_surface.b_view);
        let restir_surface_c_view = image_view(pass_data.restir_surface.c_view);
        let restir_history_surface_a_view = image_view(pass_data.restir_history_surface.a_view);
        let restir_history_surface_b_view = image_view(pass_data.restir_history_surface.b_view);
        let restir_history_surface_c_view = image_view(pass_data.restir_history_surface.c_view);

        // 同一 command buffer 中连续执行 path/temporal/spatial/final 多个 TraceRays。
        // 这些图像可能在相邻 phase 间先写后读或读写同图，因此统一纳入 ray-tracing
        // shader read/write barrier 集合；history 图像虽然只读，加入 barrier 可保持同步模板简单且安全。
        let phase_images = [
            pass_data.single_frame_output,
            pass_data.gbuffer_a,
            pass_data.gbuffer_b,
            pass_data.gbuffer_c,
            pass_data.motion_vectors,
            pass_data.restir_initial.a,
            pass_data.restir_initial.b,
            pass_data.restir_initial.c,
            pass_data.restir_initial.d,
            pass_data.restir_temporal.a,
            pass_data.restir_temporal.b,
            pass_data.restir_temporal.c,
            pass_data.restir_temporal.d,
            pass_data.restir_final.a,
            pass_data.restir_final.b,
            pass_data.restir_final.c,
            pass_data.restir_final.d,
            pass_data.restir_history.a,
            pass_data.restir_history.b,
            pass_data.restir_history.c,
            pass_data.restir_history.d,
            pass_data.restir_surface.a,
            pass_data.restir_surface.b,
            pass_data.restir_surface.c,
            pass_data.restir_history_surface.a,
            pass_data.restir_history_surface.b,
            pass_data.restir_history_surface.c,
        ]
        .into_iter()
        .map(image)
        .collect_vec();

        cmd.begin_label("Ray trace", glam::vec4(0.0, 1.0, 0.0, 1.0));

        cmd.cmd_bind_pipeline(vk::PipelineBindPoint::RAY_TRACING_KHR, self.pipeline.pipeline);

        macro_rules! image_info {
            ($view:expr) => {
                vec![vk::DescriptorImageInfo::default().image_layout(vk::ImageLayout::GENERAL).image_view($view)]
            };
        }

        cmd.push_descriptor_set(
            vk::PipelineBindPoint::RAY_TRACING_KHR,
            self.pipeline.pipeline_layout,
            gpu::REALTIME_RT_SET_NUM,
            &[
                RealtimeRtDescriptorBinding::tlas().write_tals(vk::DescriptorSet::null(), 0, vec![tlas]),
                RealtimeRtDescriptorBinding::rt_single_frame_output().write_image(
                    vk::DescriptorSet::null(),
                    0,
                    image_info!(rt_image_view),
                ),
                RealtimeRtDescriptorBinding::gbuffer_a().write_image(
                    vk::DescriptorSet::null(),
                    0,
                    image_info!(gbuffer_a_view),
                ),
                RealtimeRtDescriptorBinding::gbuffer_b().write_image(
                    vk::DescriptorSet::null(),
                    0,
                    image_info!(gbuffer_b_view),
                ),
                RealtimeRtDescriptorBinding::gbuffer_c().write_image(
                    vk::DescriptorSet::null(),
                    0,
                    image_info!(gbuffer_c_view),
                ),
                RealtimeRtDescriptorBinding::dlss_depth().write_image(
                    vk::DescriptorSet::null(),
                    0,
                    image_info!(depth_view),
                ),
                RealtimeRtDescriptorBinding::dlss_motion_vectors().write_image(
                    vk::DescriptorSet::null(),
                    0,
                    image_info!(motion_vectors_view),
                ),
                RealtimeRtDescriptorBinding::dlss_rr_diffuse_albedo().write_image(
                    vk::DescriptorSet::null(),
                    0,
                    image_info!(rr_diffuse_albedo_view),
                ),
                RealtimeRtDescriptorBinding::dlss_rr_specular_albedo().write_image(
                    vk::DescriptorSet::null(),
                    0,
                    image_info!(rr_specular_albedo_view),
                ),
                RealtimeRtDescriptorBinding::dlss_rr_specular_motion_vectors().write_image(
                    vk::DescriptorSet::null(),
                    0,
                    image_info!(rr_specular_motion_vectors_view),
                ),
                RealtimeRtDescriptorBinding::restir_initial_a().write_image(
                    vk::DescriptorSet::null(),
                    0,
                    image_info!(restir_initial_a_view),
                ),
                RealtimeRtDescriptorBinding::restir_initial_b().write_image(
                    vk::DescriptorSet::null(),
                    0,
                    image_info!(restir_initial_b_view),
                ),
                RealtimeRtDescriptorBinding::restir_initial_c().write_image(
                    vk::DescriptorSet::null(),
                    0,
                    image_info!(restir_initial_c_view),
                ),
                RealtimeRtDescriptorBinding::restir_initial_d().write_image(
                    vk::DescriptorSet::null(),
                    0,
                    image_info!(restir_initial_d_view),
                ),
                RealtimeRtDescriptorBinding::restir_temporal_a().write_image(
                    vk::DescriptorSet::null(),
                    0,
                    image_info!(restir_temporal_a_view),
                ),
                RealtimeRtDescriptorBinding::restir_temporal_b().write_image(
                    vk::DescriptorSet::null(),
                    0,
                    image_info!(restir_temporal_b_view),
                ),
                RealtimeRtDescriptorBinding::restir_temporal_c().write_image(
                    vk::DescriptorSet::null(),
                    0,
                    image_info!(restir_temporal_c_view),
                ),
                RealtimeRtDescriptorBinding::restir_temporal_d().write_image(
                    vk::DescriptorSet::null(),
                    0,
                    image_info!(restir_temporal_d_view),
                ),
                RealtimeRtDescriptorBinding::restir_final_a().write_image(
                    vk::DescriptorSet::null(),
                    0,
                    image_info!(restir_final_a_view),
                ),
                RealtimeRtDescriptorBinding::restir_final_b().write_image(
                    vk::DescriptorSet::null(),
                    0,
                    image_info!(restir_final_b_view),
                ),
                RealtimeRtDescriptorBinding::restir_final_c().write_image(
                    vk::DescriptorSet::null(),
                    0,
                    image_info!(restir_final_c_view),
                ),
                RealtimeRtDescriptorBinding::restir_final_d().write_image(
                    vk::DescriptorSet::null(),
                    0,
                    image_info!(restir_final_d_view),
                ),
                RealtimeRtDescriptorBinding::restir_history_a().write_image(
                    vk::DescriptorSet::null(),
                    0,
                    image_info!(restir_history_a_view),
                ),
                RealtimeRtDescriptorBinding::restir_history_b().write_image(
                    vk::DescriptorSet::null(),
                    0,
                    image_info!(restir_history_b_view),
                ),
                RealtimeRtDescriptorBinding::restir_history_c().write_image(
                    vk::DescriptorSet::null(),
                    0,
                    image_info!(restir_history_c_view),
                ),
                RealtimeRtDescriptorBinding::restir_history_d().write_image(
                    vk::DescriptorSet::null(),
                    0,
                    image_info!(restir_history_d_view),
                ),
                RealtimeRtDescriptorBinding::restir_surface_a().write_image(
                    vk::DescriptorSet::null(),
                    0,
                    image_info!(restir_surface_a_view),
                ),
                RealtimeRtDescriptorBinding::restir_surface_b().write_image(
                    vk::DescriptorSet::null(),
                    0,
                    image_info!(restir_surface_b_view),
                ),
                RealtimeRtDescriptorBinding::restir_surface_c().write_image(
                    vk::DescriptorSet::null(),
                    0,
                    image_info!(restir_surface_c_view),
                ),
                RealtimeRtDescriptorBinding::restir_history_surface_a().write_image(
                    vk::DescriptorSet::null(),
                    0,
                    image_info!(restir_history_surface_a_view),
                ),
                RealtimeRtDescriptorBinding::restir_history_surface_b().write_image(
                    vk::DescriptorSet::null(),
                    0,
                    image_info!(restir_history_surface_b_view),
                ),
                RealtimeRtDescriptorBinding::restir_history_surface_c().write_image(
                    vk::DescriptorSet::null(),
                    0,
                    image_info!(restir_history_surface_c_view),
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

        // SHARC regular descriptor set 在 pass 创建时已一次性写入持久 buffer，这里每帧只 bind 到
        // SHARC_SET_NUM（= REALTIME_RT_SET_NUM + 1）。即使 SHARC Off 也要 bind：raygen 静态引用了这些 buffer。
        cmd.bind_descriptor_sets(
            vk::PipelineBindPoint::RAY_TRACING_KHR,
            self.pipeline.pipeline_layout,
            gpu::REALTIME_RT_SET_NUM + 1,
            &[self.sharc_descriptor_set.handle()],
            None,
        );
        // FIXME 这个变量废除了，现在只有 spp 1
        let spp = 1;
        let mut push_constant = gpu::realtime_rt::PushConstants {
            spp_idx: 0,
            channel: pass_data.debug_channel,
            sky_sampling_mode: pass_data.sky_sampling_mode,
            sky_brightness: pass_data.sky_brightness,
            emissive_nee_enabled: u32::from(pass_data.emissive_nee_enabled),
            analytic_nee_enabled: u32::from(pass_data.analytic_nee_enabled),
            restir_di_mode: pass_data.restir_di_mode,
            restir_di_phase: RESTIR_DI_PHASE_PATH,
            restir_history_valid: u32::from(pass_data.restir_history_valid),
            sharc_mode: pass_data.sharc_mode,
            sharc_phase: SHARC_PHASE_NONE,
            sharc_capacity: pass_data.sharc_capacity,
            sharc_scene_scale: pass_data.sharc_scene_scale,
            _padding_0: 0,
            _padding_1: 0,
            _padding_2: 0,
        };

        let trace_extent = [
            pass_data.single_frame_extent.width,
            pass_data.single_frame_extent.height,
            1,
        ];
        let shader_stages = vk::ShaderStageFlags::RAYGEN_KHR
            | vk::ShaderStageFlags::MISS_KHR
            | vk::ShaderStageFlags::ANY_HIT_KHR
            | vk::ShaderStageFlags::CLOSEST_HIT_KHR;

        let push_and_trace = |push_constant: &gpu::realtime_rt::PushConstants, extent: [u32; 3]| {
            cmd.cmd_push_constants(
                self.pipeline.pipeline_layout,
                shader_stages,
                0,
                BytesConvert::bytes_of(push_constant),
            );

            cmd.trace_rays(
                self.sbt.raygen_region(),
                self.sbt.miss_region(),
                self.sbt.hit_region(),
                self.sbt.callable_region(),
                extent,
            );
        };

        // SHARC buffer 同步：Update 写 hash/accumulation，Resolve 读 accumulation + 读写 resolved，
        // 两者之间用 RT shader 读写 global memory barrier 串联。跨帧的持久 buffer 复用依赖 SHARC
        // 自身对原子竞争的容忍以及帧间提交顺序，这里只保证 pass 内 Update→Resolve 的可见性。
        let barrier_sharc_buffers = || {
            cmd.memory_barrier(&[vk::MemoryBarrier2::default()
                .src_stage_mask(vk::PipelineStageFlags2::RAY_TRACING_SHADER_KHR)
                .src_access_mask(vk::AccessFlags2::SHADER_WRITE | vk::AccessFlags2::SHADER_READ)
                .dst_stage_mask(vk::PipelineStageFlags2::RAY_TRACING_SHADER_KHR)
                .dst_access_mask(vk::AccessFlags2::SHADER_READ | vk::AccessFlags2::SHADER_WRITE)]);
        };

        let barrier_phase_images = || {
            let image_barriers = phase_images
                .iter()
                .map(|image| {
                    GfxImageBarrier::new()
                        .image(*image)
                        .image_aspect_flag(vk::ImageAspectFlags::COLOR)
                        .src_mask(
                            vk::PipelineStageFlags2::RAY_TRACING_SHADER_KHR,
                            vk::AccessFlags2::SHADER_WRITE | vk::AccessFlags2::SHADER_READ,
                        )
                        .dst_mask(
                            vk::PipelineStageFlags2::RAY_TRACING_SHADER_KHR,
                            vk::AccessFlags2::SHADER_READ | vk::AccessFlags2::SHADER_WRITE,
                        )
                })
                .collect_vec();

            cmd.image_memory_barrier(vk::DependencyFlags::empty(), &image_barriers);
        };

        // SHARC 维护 sub-pass：在正常 path / ReSTIR 之前 Update→Resolve 维护世界空间缓存。
        // stage 8b 不查询缓存，因此这一步不改变主渲染结果，只填充 / 合并 / 淘汰缓存。
        if pass_data.sharc_mode != SHARC_MODE_OFF {
            // Resolve 用 2D 网格覆盖 capacity：width 固定、height 上取整；多余线程在 shader 内按 entry index 越界返回。
            let resolve_width = SHARC_RESOLVE_DISPATCH_WIDTH.min(pass_data.sharc_capacity.max(1));
            let resolve_height = pass_data.sharc_capacity.div_ceil(resolve_width);
            let sharc_resolve_extent = [resolve_width, resolve_height, 1];

            // Update：在 render extent 上稀疏选像素跑独立 path，写 hash / accumulation。
            push_constant.sharc_phase = SHARC_PHASE_UPDATE;
            push_and_trace(&push_constant, trace_extent);
            barrier_sharc_buffers();

            // Resolve：按 entry 合并本帧 accumulation 到 resolved，并淘汰 stale entry、清空 accumulation。
            push_constant.sharc_phase = SHARC_PHASE_RESOLVE;
            push_and_trace(&push_constant, sharc_resolve_extent);
            barrier_sharc_buffers();

            push_constant.sharc_phase = SHARC_PHASE_NONE;
        }

        for spp_idx in 0..spp {
            push_constant.spp_idx = spp_idx;
            push_constant.restir_di_phase = RESTIR_DI_PHASE_PATH;

            // 在 spp 之间，需要插入一个 image barrier，确保上一次的写入被下一次读取到。
            if spp_idx != 0 {
                barrier_phase_images();
            }

            push_and_trace(&push_constant, trace_extent);

            if pass_data.restir_di_mode == RESTIR_DI_MODE_OFF {
                continue;
            }

            // ReSTIR DI 的 path phase 只生成 initial reservoir 和 surface key；temporal/spatial/final
            // 都依赖这些 storage image 的结果，因此每个 phase 之间必须显式插入 shader barrier。
            barrier_phase_images();
            if pass_data.restir_di_mode != RESTIR_DI_MODE_INITIAL_ONLY {
                push_constant.restir_di_phase = RESTIR_DI_PHASE_TEMPORAL;
                push_and_trace(&push_constant, trace_extent);
                barrier_phase_images();

                if pass_data.restir_di_mode == RESTIR_DI_MODE_TEMPORAL_SPATIAL {
                    push_constant.restir_di_phase = RESTIR_DI_PHASE_SPATIAL;
                    push_and_trace(&push_constant, trace_extent);
                    barrier_phase_images();
                }
            }

            push_constant.restir_di_phase = RESTIR_DI_PHASE_FINAL;
            push_and_trace(&push_constant, trace_extent);
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
    pub emissive_nee_enabled: bool,
    pub analytic_nee_enabled: bool,
    pub restir_di_mode: u32,
    pub restir_history_valid: bool,

    // ========== SHARC world-space radiance cache ==========
    // SHARC buffer 不进入 RenderGraph（它只跟踪 image），也不在这里传句柄：buffer 是持久资源，
    // 已在 pass 创建时写入 SHARC regular descriptor set。这里只传每帧可调的 push constant 参数。
    pub sharc_mode: u32,
    pub sharc_capacity: u32,
    pub sharc_scene_scale: f32,

    // ========== ReSTIR DI 数据 ==========
    pub restir_initial: RestirReservoirRgImages,
    pub restir_temporal: RestirReservoirRgImages,
    pub restir_final: RestirReservoirRgImages,
    pub restir_history: RestirReservoirRgImages,
    pub restir_surface: RestirSurfaceKeyRgImages,
    pub restir_history_surface: RestirSurfaceKeyRgImages,

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
        // ReSTIR DI 开启时同一 pass 内的后续 TraceRays phase 会读取 GBuffer/mvec/HDR，
        // 因此这些图像在 RenderGraph 视角声明为 ray tracing storage read/write。
        builder.read_write_image(self.single_frame_image, RgImageState::STORAGE_READ_WRITE_RAY_TRACING);
        builder.read_write_image(self.gbuffer_a, RgImageState::STORAGE_READ_WRITE_RAY_TRACING);
        builder.read_write_image(self.gbuffer_b, RgImageState::STORAGE_READ_WRITE_RAY_TRACING);
        builder.read_write_image(self.gbuffer_c, RgImageState::STORAGE_READ_WRITE_RAY_TRACING);
        builder.write_image(self.depth, RgImageState::STORAGE_WRITE_RAY_TRACING);
        builder.read_write_image(self.motion_vectors, RgImageState::STORAGE_READ_WRITE_RAY_TRACING);
        builder.write_image(self.rr_diffuse_albedo, RgImageState::STORAGE_WRITE_RAY_TRACING);
        builder.write_image(self.rr_specular_albedo, RgImageState::STORAGE_WRITE_RAY_TRACING);
        builder.write_image(self.rr_specular_motion_vectors, RgImageState::STORAGE_WRITE_RAY_TRACING);

        setup_reservoir_images(builder, self.restir_initial);
        setup_reservoir_images(builder, self.restir_temporal);
        setup_reservoir_images(builder, self.restir_final);
        setup_reservoir_images(builder, self.restir_history);
        setup_surface_key_images(builder, self.restir_surface);
        setup_surface_key_images(builder, self.restir_history_surface);
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

        let restir_initial = reservoir_pass_images(ctx, self.restir_initial, "restir_initial");
        let restir_temporal = reservoir_pass_images(ctx, self.restir_temporal, "restir_temporal");
        let restir_final = reservoir_pass_images(ctx, self.restir_final, "restir_final");
        let restir_history = reservoir_pass_images(ctx, self.restir_history, "restir_history");
        let restir_surface = surface_key_pass_images(ctx, self.restir_surface, "restir_surface");
        let restir_history_surface =
            surface_key_pass_images(ctx, self.restir_history_surface, "restir_history_surface");

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
                emissive_nee_enabled: self.emissive_nee_enabled,
                analytic_nee_enabled: self.analytic_nee_enabled,
                restir_di_mode: self.restir_di_mode,
                restir_history_valid: self.restir_history_valid,
                sharc_mode: self.sharc_mode,
                sharc_capacity: self.sharc_capacity,
                sharc_scene_scale: self.sharc_scene_scale,
                restir_initial,
                restir_temporal,
                restir_final,
                restir_history,
                restir_surface,
                restir_history_surface,
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

fn setup_reservoir_images(builder: &mut RgPassBuilder, images: RestirReservoirRgImages) {
    builder.read_write_image(images.a, RgImageState::STORAGE_READ_WRITE_RAY_TRACING);
    builder.read_write_image(images.b, RgImageState::STORAGE_READ_WRITE_RAY_TRACING);
    builder.read_write_image(images.c, RgImageState::STORAGE_READ_WRITE_RAY_TRACING);
    builder.read_write_image(images.d, RgImageState::STORAGE_READ_WRITE_RAY_TRACING);
}

fn setup_surface_key_images(builder: &mut RgPassBuilder, images: RestirSurfaceKeyRgImages) {
    builder.read_write_image(images.a, RgImageState::STORAGE_READ_WRITE_RAY_TRACING);
    builder.read_write_image(images.b, RgImageState::STORAGE_READ_WRITE_RAY_TRACING);
    builder.read_write_image(images.c, RgImageState::STORAGE_READ_WRITE_RAY_TRACING);
}

fn reservoir_pass_images(
    ctx: &RgPassContext<'_>,
    images: RestirReservoirRgImages,
    label: &str,
) -> RestirReservoirPassImages {
    let (a, a_view) =
        ctx.get_image_and_view_handle(images.a).unwrap_or_else(|| panic!("RealtimeRtRgPass: {label}.a not found"));
    let (b, b_view) =
        ctx.get_image_and_view_handle(images.b).unwrap_or_else(|| panic!("RealtimeRtRgPass: {label}.b not found"));
    let (c, c_view) =
        ctx.get_image_and_view_handle(images.c).unwrap_or_else(|| panic!("RealtimeRtRgPass: {label}.c not found"));
    let (d, d_view) =
        ctx.get_image_and_view_handle(images.d).unwrap_or_else(|| panic!("RealtimeRtRgPass: {label}.d not found"));

    RestirReservoirPassImages {
        a,
        a_view,
        b,
        b_view,
        c,
        c_view,
        d,
        d_view,
    }
}

fn surface_key_pass_images(
    ctx: &RgPassContext<'_>,
    images: RestirSurfaceKeyRgImages,
    label: &str,
) -> RestirSurfaceKeyPassImages {
    let (a, a_view) =
        ctx.get_image_and_view_handle(images.a).unwrap_or_else(|| panic!("RealtimeRtRgPass: {label}.a not found"));
    let (b, b_view) =
        ctx.get_image_and_view_handle(images.b).unwrap_or_else(|| panic!("RealtimeRtRgPass: {label}.b not found"));
    let (c, c_view) =
        ctx.get_image_and_view_handle(images.c).unwrap_or_else(|| panic!("RealtimeRtRgPass: {label}.c not found"));

    RestirSurfaceKeyPassImages {
        a,
        a_view,
        b,
        b_view,
        c,
        c_view,
    }
}
