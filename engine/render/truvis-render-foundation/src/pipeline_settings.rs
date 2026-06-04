use std::{fmt::Display, ops::Deref};

use ash::vk;

use crate::render_view::RenderViewAccumSignature;

/// 渲染运行时默认配置
pub struct DefaultRenderRuntimeSettings;
impl DefaultRenderRuntimeSettings {
    pub const DEFAULT_SURFACE_FORMAT: vk::SurfaceFormatKHR = vk::SurfaceFormatKHR {
        // shader 输出会被自动改变： liner -> sRGB
        format: vk::Format::R8G8B8A8_SRGB,
        // 通知 OS，将数值按照 sRGB 空间进行处理和显示
        color_space: vk::ColorSpaceKHR::SRGB_NONLINEAR,
    };
    pub const DEFAULT_PRESENT_MODE: vk::PresentModeKHR = vk::PresentModeKHR::MAILBOX;
    pub const DEPTH_FORMAT_CANDIDATES: &'static [vk::Format] = &[
        vk::Format::D32_SFLOAT_S8_UINT,
        vk::Format::D32_SFLOAT,
        vk::Format::D24_UNORM_S8_UINT,
        vk::Format::D16_UNORM_S8_UINT,
        vk::Format::D16_UNORM,
    ];
}

/// 帧级渲染配置
///
/// DLSS 接入后，一帧需要同时描述“实际渲染尺寸”和“最终输出尺寸”：
/// RT/GBuffer/DLSS input 按 `render_extent` 创建，present/GUI/main-view 按
/// `output_extent` 创建。Native/DLAA/fallback 路径会让两者相等。
#[derive(Copy, Clone, Default, PartialEq, Eq)]
pub struct FrameSettings {
    pub color_format: vk::Format,
    pub depth_format: vk::Format,
    /// RT、GBuffer、motion vector 等低分辨率渲染资源的尺寸。
    pub render_extent: vk::Extent2D,
    /// present、GUI 和最终离屏 color 的输出尺寸，通常等于 swapchain extent。
    pub output_extent: vk::Extent2D,
}

impl FrameSettings {
    /// 当前是否处于原生分辨率路径。
    #[inline]
    pub fn is_native_extent(self) -> bool {
        self.render_extent == self.output_extent
    }

    /// Native / fallback 路径使用同一尺寸，避免 SR 关闭时保留旧的低分辨率状态。
    #[inline]
    pub fn set_native_extent(&mut self, extent: vk::Extent2D) {
        self.render_extent = extent;
        self.output_extent = extent;
    }
}

/// DLSS Super Resolution / DLAA 模式。
///
/// 这里只表示 `kFeatureDLSS` 的模式选择；Ray Reconstruction 后续作为独立开关，
/// 在执行层替换 SR evaluate，而不是作为这里的另一个互斥质量模式。
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DlssSrMode {
    Off,
    Dlaa,
    Quality,
    Balanced,
    Performance,
    UltraPerformance,
}

impl Default for DlssSrMode {
    fn default() -> Self {
        Self::Off
    }
}

impl DlssSrMode {
    pub const ALL: [Self; 6] = [
        Self::Off,
        Self::Dlaa,
        Self::Quality,
        Self::Balanced,
        Self::Performance,
        Self::UltraPerformance,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Dlaa => "DLAA",
            Self::Quality => "Quality",
            Self::Balanced => "Balanced",
            Self::Performance => "Performance",
            Self::UltraPerformance => "Ultra Performance",
        }
    }

    /// 解析调试启动配置中的 DLSS SR 模式名称。
    ///
    /// 允许空格、连字符和下划线差异，是为了让环境变量输入对大小写和写法宽容。
    pub fn from_config_value(value: &str) -> Option<Self> {
        let normalized = value
            .trim()
            .chars()
            .filter(|ch| !matches!(ch, ' ' | '-' | '_'))
            .flat_map(char::to_lowercase)
            .collect::<String>();

        match normalized.as_str() {
            "off" => Some(Self::Off),
            "dlaa" => Some(Self::Dlaa),
            "quality" => Some(Self::Quality),
            "balanced" => Some(Self::Balanced),
            "performance" => Some(Self::Performance),
            "ultraperformance" => Some(Self::UltraPerformance),
            _ => None,
        }
    }
}

/// 降噪设置
#[derive(Copy, Clone)]
pub struct DenoiseSettings {
    /// 是否启用降噪
    pub enabled: bool,
    /// 颜色差异的 sigma 参数（控制颜色相似度权重）
    pub sigma_color: f32,
    /// 深度差异的 sigma 参数（控制深度相似度权重）
    pub sigma_depth: f32,
    /// 法线差异的 sigma 参数（控制法线相似度权重）
    pub sigma_normal: f32,
    /// 滤波核半径（1-5）
    pub kernel_radius: i32,

    // ========== 增强联合双边滤波参数 ==========
    /// Albedo 差异的 sigma 参数（控制材质相似度权重）
    pub sigma_albedo: f32,
    /// 世界空间位置差异的 sigma 参数（归一化到 scene_scale）
    pub sigma_position: f32,
    /// 场景尺度（用于归一化世界空间距离，如 Cornell Box 约 400）
    pub scene_scale: f32,

    // ========== 粗糙度自适应参数 ==========
    /// 是否启用粗糙度自适应滤波
    pub roughness_adaptive_enabled: bool,
    /// 粗糙度对滤波半径的影响因子（roughness=1 时半径放大倍数）
    pub roughness_radius_scale: f32,
    /// 粗糙度对 sigma_normal 的影响因子（roughness=1 时 sigma 放大倍数）
    pub roughness_sigma_scale: f32,
}

impl Default for DenoiseSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            sigma_color: 0.1,
            sigma_depth: 1.0, // 提高默认值适应大场景
            sigma_normal: 0.5,
            kernel_radius: 3, // 提高默认值提升降噪效果

            // 增强联合双边滤波参数
            sigma_albedo: 0.1,
            sigma_position: 0.1,
            scene_scale: 400.0, // Cornell Box 尺度

            // 粗糙度自适应参数
            roughness_adaptive_enabled: true,
            roughness_radius_scale: 2.0,
            roughness_sigma_scale: 1.5,
        }
    }
}

/// 管线级配置
#[derive(Copy, Clone)]
pub struct PipelineSettings {
    /// 0 表示 RT，1 表示 normal
    pub channel: u32,
    /// DLSS SR / DLAA 模式。RR 后续作为独立开关接入，不和这里的质量模式平级。
    pub dlss_sr_mode: DlssSrMode,
    /// 降噪设置
    pub denoise: DenoiseSettings,
    /// 是否启用 Irradiance Cache
    pub ic_enabled: bool,
}

impl Default for PipelineSettings {
    fn default() -> Self {
        Self {
            channel: 0,
            dlss_sr_mode: DlssSrMode::Off,
            denoise: DenoiseSettings::default(),
            // 主流程当前不再依赖 Irradiance Cache；代码保留用于后续实验或 debug 通道。
            ic_enabled: false,
        }
    }
}

/// 呈现配置
#[derive(Copy, Clone)]
pub struct PresentSettings {
    pub canvas_extent: vk::Extent2D,

    pub swapchain_image_cnt: usize,
    pub color_format: vk::Format,
}

/// 帧标签（A/B/C）
///
/// 表示当前处于 Frames in Flight 的哪一帧。
/// 通过 `Deref` 转换为索引 0/1/2。
#[derive(Debug, Clone, Copy)]
pub enum FrameLabel {
    A,
    B,
    C,
}
impl Deref for FrameLabel {
    type Target = usize;
    #[inline]
    fn deref(&self) -> &Self::Target {
        match self {
            Self::A => &Self::INDEX[0],
            Self::B => &Self::INDEX[1],
            Self::C => &Self::INDEX[2],
        }
    }
}
impl Display for FrameLabel {
    #[inline]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::A => write!(f, "A"),
            Self::B => write!(f, "B"),
            Self::C => write!(f, "C"),
        }
    }
}
impl FrameLabel {
    const INDEX: [usize; 3] = [0, 1, 2];

    #[inline]
    pub fn from_usize(idx: usize) -> Self {
        match idx {
            0 => Self::A,
            1 => Self::B,
            2 => Self::C,
            _ => panic!("Invalid frame index: {idx}"),
        }
    }
}

/// 用于逐帧累积的数据
#[derive(Copy, Clone, Default)]
pub struct AccumData {
    last_render_view: Option<RenderViewAccumSignature>,

    accum_frames_num: usize,
}
impl AccumData {
    /// 调用阶段：BeforeRender-CollectData
    pub fn update_accum_frames(&mut self, render_view: RenderViewAccumSignature) {
        if self.last_render_view != Some(render_view) {
            self.accum_frames_num = 0;
        } else {
            self.accum_frames_num += 1;
        }

        self.last_render_view = Some(render_view);
    }

    pub fn reset(&mut self) {
        self.last_render_view = None;
        self.accum_frames_num = 0;
    }

    #[inline]
    pub fn accum_frames_num(&self) -> usize {
        self.accum_frames_num
    }
}
