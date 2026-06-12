//! DLSS SR 的 Streamline C ABI 封装。
//!
//! 本模块保持在 raw Vulkan handle 层，不依赖 `ash`。调用方负责保证这些 handle
//! 来自已经被 Streamline interposer 看到的 Vulkan root，并且资源生命周期覆盖 evaluate。

use crate::{runtime::StreamlineError, truvixx};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Rust 侧暴露的 SR/DLAA 模式。
///
/// 该枚举只表达 Streamline DLSS SR 的 Performance Quality Mode，不包含 RR 开关。
/// RR 后续应作为独立 enable flag，而不是扩展到这里成为另一个质量挡位。
pub enum DlssMode {
    Off,
    Dlaa,
    Quality,
    Balanced,
    Performance,
    UltraPerformance,
}

impl DlssMode {
    fn to_ffi(self) -> u32 {
        match self {
            Self::Off => truvixx::TruvixxSlDlssMode_TruvixxSlDlssModeOff,
            Self::Dlaa => truvixx::TruvixxSlDlssMode_TruvixxSlDlssModeDlaa,
            Self::Quality => truvixx::TruvixxSlDlssMode_TruvixxSlDlssModeMaxQuality,
            Self::Balanced => truvixx::TruvixxSlDlssMode_TruvixxSlDlssModeBalanced,
            Self::Performance => truvixx::TruvixxSlDlssMode_TruvixxSlDlssModeMaxPerformance,
            Self::UltraPerformance => truvixx::TruvixxSlDlssMode_TruvixxSlDlssModeUltraPerformance,
        }
    }
}

#[derive(Clone, Copy, Debug)]
/// `slDLSSSetOptions` / `slDLSSGetOptimalSettings` 共用的最小 options。
///
/// 这里故意只暴露当前 SR 接入需要的字段，pre-exposure、auto exposure、alpha upscaling
/// 等策略先固定在 C ABI 转换层，避免 app 层过早承诺未验证的配置面。
pub struct DlssOptions {
    pub mode: DlssMode,
    pub output_width: u32,
    pub output_height: u32,
    pub color_buffers_hdr: bool,
}

impl DlssOptions {
    fn to_ffi(self) -> truvixx::TruvixxSlDlssOptions {
        truvixx::TruvixxSlDlssOptions {
            mode: self.mode.to_ffi(),
            output_width: self.output_width,
            output_height: self.output_height,
            pre_exposure: 1.0,
            exposure_scale: 1.0,
            color_buffers_hdr: u32::from(self.color_buffers_hdr),
        }
    }
}

#[derive(Clone, Copy, Debug)]
/// `slDLSSDSetOptions` / `slDLSSDGetOptimalSettings` 共用的最小 RR options。
///
/// RR 使用与 DLSS SR 相同的 Performance Quality Mode，但通过 `kFeatureDLSS_RR`
/// 独立 evaluate。当前管线把 roughness 打包在 normal 的 alpha 通道中。
pub struct DlssRrOptions {
    pub mode: DlssMode,
    pub output_width: u32,
    pub output_height: u32,
    pub color_buffers_hdr: bool,
    pub normal_roughness_packed: bool,
    pub world_to_camera_view: [f32; 16],
    pub camera_view_to_world: [f32; 16],
}

impl DlssRrOptions {
    fn to_ffi(self) -> truvixx::TruvixxSlDlssRrOptions {
        truvixx::TruvixxSlDlssRrOptions {
            mode: self.mode.to_ffi(),
            output_width: self.output_width,
            output_height: self.output_height,
            pre_exposure: 1.0,
            exposure_scale: 1.0,
            color_buffers_hdr: u32::from(self.color_buffers_hdr),
            normal_roughness_packed: u32::from(self.normal_roughness_packed),
            world_to_camera_view: self.world_to_camera_view,
            camera_view_to_world: self.camera_view_to_world,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
/// Streamline 为目标输出尺寸和 SR mode 推荐的低分辨率渲染尺寸。
///
/// Runtime 用 `optimal_render_width/height` 重建 RT/GBuffer/DLSS input targets；
/// min/max 目前只打日志，用于后续 UI 或异常诊断。
pub struct DlssOptimalSettings {
    pub optimal_render_width: u32,
    pub optimal_render_height: u32,
    pub optimal_sharpness: f32,
    pub render_width_min: u32,
    pub render_height_min: u32,
    pub render_width_max: u32,
    pub render_height_max: u32,
}

impl From<truvixx::TruvixxSlDlssOptimalSettings> for DlssOptimalSettings {
    fn from(value: truvixx::TruvixxSlDlssOptimalSettings) -> Self {
        Self {
            optimal_render_width: value.optimal_render_width,
            optimal_render_height: value.optimal_render_height,
            optimal_sharpness: value.optimal_sharpness,
            render_width_min: value.render_width_min,
            render_height_min: value.render_height_min,
            render_width_max: value.render_width_max,
            render_height_max: value.render_height_max,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
/// DLSS feature support 与 requirements 查询结果。
///
/// `supported=false` 不代表 runtime 初始化失败，只表示当前 adapter/driver/feature 组合不支持
/// 对应 DLSS feature；调用方应降级到 native path，而不是让进程启动失败。
pub struct DlssSupport {
    pub supported: bool,
    pub flags: u32,
    pub max_num_viewports: u32,
    pub max_num_cpu_threads: u32,
}

#[derive(Clone, Copy, Debug, Default)]
/// 非 proxy 集成方式使用的 Vulkan root 信息。
///
/// 当前 Truvis 生产路径通过 `sl.interposer.dll` 创建 Vulkan entry，因此 runtime 不主动调用
/// `set_vulkan_info`。保留该结构是为了未来切换到手动 hook/非 proxy 路径时不用改 ABI。
pub struct VulkanInfo {
    pub instance: u64,
    pub physical_device: u64,
    pub device: u64,
    pub graphics_queue_family: u32,
    pub graphics_queue_index: u32,
    pub compute_queue_family: u32,
    pub compute_queue_index: u32,
}

#[derive(Clone, Copy, Debug, Default)]
/// Streamline resource tag 所需的 Vulkan 图像快照。
///
/// 所有 handle 都是 raw Vulkan handle，layout/format/usage 必须反映录制该帧 command buffer 时的
/// 真实状态。调用方负责保证 image/view/memory 至少存活到 evaluate 与后续使用完成。
pub struct ImageResource {
    pub image: u64,
    pub memory: u64,
    pub image_view: u64,
    pub layout: u32,
    pub format: u32,
    pub width: u32,
    pub height: u32,
    pub mip_levels: u32,
    pub array_layers: u32,
    pub flags: u32,
    pub usage: u32,
}

impl ImageResource {
    fn to_ffi(self) -> truvixx::TruvixxSlImageResource {
        truvixx::TruvixxSlImageResource {
            image: self.image,
            memory: self.memory,
            image_view: self.image_view,
            layout: self.layout,
            format: self.format,
            width: self.width,
            height: self.height,
            mip_levels: self.mip_levels,
            array_layers: self.array_layers,
            flags: self.flags,
            usage: self.usage,
        }
    }
}

#[derive(Clone, Copy, Debug)]
/// Streamline common constants 的 Rust POD 镜像。
///
/// 矩阵已在 foundation 层转成 Streamline 期望的 row-major 布局；本层只做 ABI 搬运，
/// 不重新解释坐标系、jitter 或 motion vector 语义。
pub struct Constants {
    pub camera_view_to_clip: [f32; 16],
    pub clip_to_camera_view: [f32; 16],
    pub clip_to_prev_clip: [f32; 16],
    pub prev_clip_to_clip: [f32; 16],
    pub jitter_offset: [f32; 2],
    pub mvec_scale: [f32; 2],
    pub camera_pos: [f32; 3],
    pub camera_up: [f32; 3],
    pub camera_right: [f32; 3],
    pub camera_fwd: [f32; 3],
    pub camera_near: f32,
    pub camera_far: f32,
    pub camera_fov: f32,
    pub camera_aspect_ratio: f32,
    pub motion_vectors_invalid_value: f32,
    pub depth_inverted: bool,
    pub camera_motion_included: bool,
    pub motion_vectors_3d: bool,
    pub reset: bool,
}

impl Constants {
    fn to_ffi(self) -> truvixx::TruvixxSlConstants {
        truvixx::TruvixxSlConstants {
            camera_view_to_clip: self.camera_view_to_clip,
            clip_to_camera_view: self.clip_to_camera_view,
            clip_to_prev_clip: self.clip_to_prev_clip,
            prev_clip_to_clip: self.prev_clip_to_clip,
            jitter_offset: self.jitter_offset,
            mvec_scale: self.mvec_scale,
            camera_pos: self.camera_pos,
            camera_up: self.camera_up,
            camera_right: self.camera_right,
            camera_fwd: self.camera_fwd,
            camera_near: self.camera_near,
            camera_far: self.camera_far,
            camera_fov: self.camera_fov,
            camera_aspect_ratio: self.camera_aspect_ratio,
            motion_vectors_invalid_value: self.motion_vectors_invalid_value,
            depth_inverted: u32::from(self.depth_inverted),
            camera_motion_included: u32::from(self.camera_motion_included),
            motion_vectors_3d: u32::from(self.motion_vectors_3d),
            reset: u32::from(self.reset),
        }
    }
}

#[derive(Clone, Copy, Debug)]
/// 一次 DLSS SR evaluate 的完整描述。
///
/// `frame_index` 用于 Streamline frame token，`viewport_id` 当前由 app 固定为 0。
/// `use_linear_depth=false` 时 C++ wrapper 会把 depth resource tag 成 `kBufferTypeDepth`。
pub struct DlssEvaluateDesc {
    pub frame_index: u32,
    pub viewport_id: u32,
    pub command_buffer: u64,
    pub constants: Constants,
    pub input_color: ImageResource,
    pub output_color: ImageResource,
    pub depth_or_linear_depth: ImageResource,
    pub motion_vectors: ImageResource,
    pub use_linear_depth: bool,
}

#[derive(Clone, Copy, Debug)]
/// 一次 DLSS Ray Reconstruction evaluate 的完整描述。
///
/// 当前 Truvis RR MVP 使用 packed forward/shading normal+roughness，并提供独立 diffuse/specular albedo
/// 与 specular motion vectors。specular motion vectors 的质量由 shader 侧输入决定，本层只搬运资源。
pub struct DlssRrEvaluateDesc {
    pub frame_index: u32,
    pub viewport_id: u32,
    pub command_buffer: u64,
    pub constants: Constants,
    pub input_color: ImageResource,
    pub output_color: ImageResource,
    pub depth_or_linear_depth: ImageResource,
    pub motion_vectors: ImageResource,
    pub diffuse_albedo: ImageResource,
    pub specular_albedo: ImageResource,
    pub normal_roughness: ImageResource,
    pub specular_motion_vectors: ImageResource,
    pub use_linear_depth: bool,
}

fn check(result: i32, context: &'static str) -> Result<(), StreamlineError> {
    if result == 0 { Ok(()) } else { Err(StreamlineError::new(result, context)) }
}

pub fn set_vulkan_info(info: VulkanInfo) -> Result<(), StreamlineError> {
    let ffi = truvixx::TruvixxSlVulkanInfo {
        instance: info.instance,
        physical_device: info.physical_device,
        device: info.device,
        graphics_queue_family: info.graphics_queue_family,
        graphics_queue_index: info.graphics_queue_index,
        compute_queue_family: info.compute_queue_family,
        compute_queue_index: info.compute_queue_index,
    };
    check(unsafe { truvixx::truvixx_sl_set_vulkan_info(&ffi) }, "slSetVulkanInfo")
}

/// 查询当前物理设备是否支持 DLSS SR。
///
/// 该函数只做 capability 发现，不分配 DLSS resource；即使查询失败，调用方也可以继续 native 渲染。
pub fn query_support(physical_device: u64) -> Result<DlssSupport, StreamlineError> {
    let mut ffi = truvixx::TruvixxSlFeatureSupport::default();
    let result = unsafe { truvixx::truvixx_sl_dlss_query_support(physical_device, &mut ffi) };
    if result != 0 {
        return Err(StreamlineError::new(result, "DLSS support query"));
    }
    Ok(DlssSupport {
        supported: ffi.supported != 0,
        flags: ffi.flags,
        max_num_viewports: ffi.max_num_viewports,
        max_num_cpu_threads: ffi.max_num_cpu_threads,
    })
}

/// 查询当前物理设备是否支持 DLSS Ray Reconstruction。
pub fn query_rr_support(physical_device: u64) -> Result<DlssSupport, StreamlineError> {
    let mut ffi = truvixx::TruvixxSlFeatureSupport::default();
    let result = unsafe { truvixx::truvixx_sl_dlss_rr_query_support(physical_device, &mut ffi) };
    if result != 0 {
        return Err(StreamlineError::new(result, "DLSS RR support query"));
    }
    Ok(DlssSupport {
        supported: ffi.supported != 0,
        flags: ffi.flags,
        max_num_viewports: ffi.max_num_viewports,
        max_num_cpu_threads: ffi.max_num_cpu_threads,
    })
}

/// 根据 output extent 和 SR mode 查询推荐 render extent。
///
/// Runtime 会把失败视为 native fallback；本函数不修改 `DlssOptions`，只返回 Streamline 结果。
pub fn get_optimal_settings(options: DlssOptions) -> Result<DlssOptimalSettings, StreamlineError> {
    let ffi_options = options.to_ffi();
    let mut ffi_settings = truvixx::TruvixxSlDlssOptimalSettings::default();
    check(
        unsafe { truvixx::truvixx_sl_dlss_get_optimal_settings(&ffi_options, &mut ffi_settings) },
        "DLSS optimal settings",
    )?;
    Ok(ffi_settings.into())
}

/// 根据 output extent 和 RR mode 查询推荐 render extent。
pub fn get_rr_optimal_settings(options: DlssRrOptions) -> Result<DlssOptimalSettings, StreamlineError> {
    let ffi_options = options.to_ffi();
    let mut ffi_settings = truvixx::TruvixxSlDlssOptimalSettings::default();
    check(
        unsafe { truvixx::truvixx_sl_dlss_rr_get_optimal_settings(&ffi_options, &mut ffi_settings) },
        "DLSS RR optimal settings",
    )?;
    Ok(ffi_settings.into())
}

/// 设置当前 viewport 的 DLSS SR options。
///
/// 需要在 evaluate 前调用，且 options 的 output extent 必须与 tagged output color 一致。
pub fn set_options(viewport_id: u32, options: DlssOptions) -> Result<(), StreamlineError> {
    let ffi_options = options.to_ffi();
    check(unsafe { truvixx::truvixx_sl_dlss_set_options(viewport_id, &ffi_options) }, "DLSS set options")
}

/// 设置当前 viewport 的 DLSS Ray Reconstruction options。
pub fn set_rr_options(viewport_id: u32, options: DlssRrOptions) -> Result<(), StreamlineError> {
    let ffi_options = options.to_ffi();
    check(unsafe { truvixx::truvixx_sl_dlss_rr_set_options(viewport_id, &ffi_options) }, "DLSS RR set options")
}

/// 调用 `slEvaluateFeature(kFeatureDLSS)`。
///
/// 调用方必须已经在同一个 command buffer 上准备好 resource layout，并保证 constants 与
/// input/output extent 匹配。这里不恢复 Vulkan pipeline state，具体需求由上层 pass 控制。
pub fn evaluate(desc: DlssEvaluateDesc) -> Result<(), StreamlineError> {
    let ffi = truvixx::TruvixxSlDlssEvaluateDesc {
        frame_index: desc.frame_index,
        viewport_id: desc.viewport_id,
        command_buffer: desc.command_buffer,
        constants: desc.constants.to_ffi(),
        input_color: desc.input_color.to_ffi(),
        output_color: desc.output_color.to_ffi(),
        depth_or_linear_depth: desc.depth_or_linear_depth.to_ffi(),
        motion_vectors: desc.motion_vectors.to_ffi(),
        use_linear_depth: u32::from(desc.use_linear_depth),
    };
    check(unsafe { truvixx::truvixx_sl_dlss_evaluate(&ffi) }, "DLSS evaluate")
}

/// 调用 `slEvaluateFeature(kFeatureDLSS_RR)`。
pub fn evaluate_rr(desc: DlssRrEvaluateDesc) -> Result<(), StreamlineError> {
    let ffi = truvixx::TruvixxSlDlssRrEvaluateDesc {
        frame_index: desc.frame_index,
        viewport_id: desc.viewport_id,
        command_buffer: desc.command_buffer,
        constants: desc.constants.to_ffi(),
        input_color: desc.input_color.to_ffi(),
        output_color: desc.output_color.to_ffi(),
        depth_or_linear_depth: desc.depth_or_linear_depth.to_ffi(),
        motion_vectors: desc.motion_vectors.to_ffi(),
        diffuse_albedo: desc.diffuse_albedo.to_ffi(),
        specular_albedo: desc.specular_albedo.to_ffi(),
        normal_roughness: desc.normal_roughness.to_ffi(),
        specular_motion_vectors: desc.specular_motion_vectors.to_ffi(),
        use_linear_depth: u32::from(desc.use_linear_depth),
    };
    check(unsafe { truvixx::truvixx_sl_dlss_rr_evaluate(&ffi) }, "DLSS RR evaluate")
}

/// 释放指定 viewport 的 DLSS feature resources。
///
/// mode 切回 Off、resize fallback 或 shutdown 前应确保相关 GPU work 已完成，再调用该函数。
pub fn free_resources(viewport_id: u32) -> Result<(), StreamlineError> {
    check(unsafe { truvixx::truvixx_sl_dlss_free_resources(viewport_id) }, "DLSS free resources")
}

/// 释放指定 viewport 的 DLSS Ray Reconstruction feature resources。
pub fn free_rr_resources(viewport_id: u32) -> Result<(), StreamlineError> {
    check(unsafe { truvixx::truvixx_sl_dlss_rr_free_resources(viewport_id) }, "DLSS RR free resources")
}
