pub use renderer_render_passes::post_process::sdr::SdrToneMappingSettings;

/// realtime / offline path tracing 共享的 Renderer 层调试参数。
///
/// 这些字段会同时改变两条 path tracing 分支的 shader 输入或最终显示映射。由具体 Renderer 持有
/// 单一实例，避免 ImGui 在 realtime / offline 切换时分别修改两套 subsystem-local 状态。
#[derive(Clone, Copy)]
pub struct PathTracingCommonSettings {
    /// HDRI / sky 直接光采样模式。
    pub sky_sampling_mode: SkySamplingMode,
    /// sky radiance 倍率；只缩放光照能量，不改变 importance sampling 的 PDF。
    pub sky_brightness: f32,
    /// 是否额外启用自发光三角形 NEE。
    pub emissive_nee_enabled: bool,
    /// 是否额外启用 analytic light NEE。
    pub analytic_nee_enabled: bool,
    /// SDR 输出路径的 tone mapping 参数。
    pub tone_mapping: SdrToneMappingSettings,
}

impl Default for PathTracingCommonSettings {
    fn default() -> Self {
        Self {
            sky_sampling_mode: SkySamplingMode::Importance,
            sky_brightness: 8.0,
            emissive_nee_enabled: true,
            analytic_nee_enabled: true,
            tone_mapping: SdrToneMappingSettings::default(),
        }
    }
}

/// HDRI / sky 直接光采样模式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkySamplingMode {
    Uniform,
    Importance,
}

impl SkySamplingMode {
    pub const ALL: [Self; 2] = [Self::Importance, Self::Uniform];

    pub fn label(self) -> &'static str {
        match self {
            Self::Uniform => "uniform",
            Self::Importance => "importance",
        }
    }

    pub fn shader_mode(self) -> u32 {
        match self {
            Self::Uniform => 0,
            Self::Importance => 1,
        }
    }
}
