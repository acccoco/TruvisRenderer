use std::{fmt::Display, ops::Deref};

use ash::vk;

/// 渲染器默认配置
pub struct DefaultRendererSettings;
impl DefaultRendererSettings {
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
#[derive(Copy, Clone, Default)]
pub struct FrameSettings {
    pub color_format: vk::Format,
    pub depth_format: vk::Format,
    pub frame_extent: vk::Extent2D,
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
    /// 降噪设置
    pub denoise: DenoiseSettings,
    /// 是否启用 Irradiance Cache
    pub ic_enabled: bool,
}

impl Default for PipelineSettings {
    fn default() -> Self {
        Self {
            channel: 0,
            denoise: DenoiseSettings::default(),
            ic_enabled: true, // 默认启用 IC
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
    last_camera_pos: glam::Vec3,
    last_camera_dir: glam::Vec3,

    accum_frames_num: usize,
}
impl AccumData {
    /// call phase: BeforeRender-CollectData
    pub fn update_accum_frames(&mut self, camera_pos: glam::Vec3, camera_dir: glam::Vec3) {
        if self.last_camera_dir != camera_dir || self.last_camera_pos != camera_pos {
            self.accum_frames_num = 0;
        } else {
            self.accum_frames_num += 1;
        }

        self.last_camera_pos = camera_pos;
        self.last_camera_dir = camera_dir;
    }

    pub fn reset(&mut self) {
        self.last_camera_pos = glam::Vec3::ZERO;
        self.last_camera_dir = glam::Vec3::ZERO;
        self.accum_frames_num = 0;
    }

    #[inline]
    pub fn accum_frames_num(&self) -> usize {
        self.accum_frames_num
    }
}
