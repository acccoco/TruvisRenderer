use truvis_streamline_binding::dlss;

/// DLSS Super Resolution / DLAA 模式。
///
/// 这里只表示 `kFeatureDLSS` 的模式选择；Ray Reconstruction 作为独立开关，
/// 在执行层替换 SR evaluate，而不是作为这里的另一个互斥质量模式。
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum DlssSrMode
{
    /// 关闭 DLSS，runtime 使用 native render extent。
    #[default]
    Off,
    /// DLAA 路径仍调用 DLSS feature，但 render extent 与 output extent 相同，只做抗锯齿。
    Dlaa,
    /// 质量优先的 SR upscale mode，render extent 由 Streamline optimal settings 决定。
    Quality,
    /// 质量与性能折中 SR upscale mode。
    Balanced,
    /// 性能优先 SR upscale mode。
    Performance,
    /// 最大放大倍率 SR upscale mode，通常只适合高分辨率输出。
    UltraPerformance,
}

impl DlssSrMode
{
    pub const ALL: [Self; 6] = [
        Self::Off,
        Self::Dlaa,
        Self::Quality,
        Self::Balanced,
        Self::Performance,
        Self::UltraPerformance,
    ];

    pub fn label(self) -> &'static str
    {
        match self {
            Self::Off => "Off",
            Self::Dlaa => "DLAA",
            Self::Quality => "Quality",
            Self::Balanced => "Balanced",
            Self::Performance => "Performance",
            Self::UltraPerformance => "Ultra Performance",
        }
    }

    /// 转换为 Streamline 的 SR/DLAA quality mode。
    ///
    /// 这里是项目内唯一的 `DlssSrMode -> dlss::DlssMode` 映射入口；RR 仍复用同一
    /// quality mode，但是否走 `kFeatureDLSS_RR` 由 `DlssOptions` 决定。
    pub(super) fn to_streamline_mode(self) -> dlss::DlssMode
    {
        match self {
            Self::Off => dlss::DlssMode::Off,
            Self::Dlaa => dlss::DlssMode::Dlaa,
            Self::Quality => dlss::DlssMode::Quality,
            Self::Balanced => dlss::DlssMode::Balanced,
            Self::Performance => dlss::DlssMode::Performance,
            Self::UltraPerformance => dlss::DlssMode::UltraPerformance,
        }
    }

    /// 解析调试启动配置中的 DLSS SR 模式名称。
    ///
    /// 允许空格、连字符和下划线差异，是为了让环境变量输入对大小写和写法宽容。
    pub fn from_config_value(value: &str) -> Option<Self>
    {
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

/// 当前帧最终启用的 DLSS feature。
///
/// SR 与 RR 共享 `DlssSrMode` 的质量档位，但 Streamline 内部资源和 evaluate 入口不同；
/// DLSS owner 用这个枚举表达“需要释放哪个 feature resource / app graph 应进入哪个分支”。
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(super) enum DlssFeature
{
    SuperResolution,
    RayReconstruction,
}

/// 用户或调试 UI 可修改的 DLSS 选项。
///
/// 这个类型同时承担配置输入与 feature 决策职责：它不拥有 GPU resource，也不调用
/// Streamline API，只把 SR mode 与 RR 开关收敛成 runtime / app graph / pass 共用的
/// 单一语义入口。
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct DlssOptions
{
    /// DLSS SR / DLAA 模式。
    ///
    /// 修改后由具体 Renderer 统一解析，必要时提交新的内部 render extent，并重置 DLSS history。
    pub dlss_sr_mode: DlssSrMode,
    /// DLSS Ray Reconstruction 开关。
    ///
    /// RR 复用 SR 的质量模式与 render/output extent 约定，但执行层会替换 `kFeatureDLSS`
    /// 为 `kFeatureDLSS_RR`。当 SR mode 为 `Off` 时，该开关不会让 RT 主流程进入 RR。
    pub dlss_rr_enabled: bool,
}

impl Default for DlssOptions
{
    fn default() -> Self
    {
        Self::NATIVE
    }
}

impl DlssOptions
{
    /// Native fallback：不调用任何 DLSS evaluate。
    pub(super) const NATIVE: Self = Self {
        dlss_sr_mode: DlssSrMode::Off,
        dlss_rr_enabled: false,
    };

    #[inline]
    pub(super) fn is_dlss_active(self) -> bool
    {
        self.active_feature().is_some()
    }

    #[inline]
    pub(super) fn is_rr_active(self) -> bool
    {
        self.active_feature() == Some(DlssFeature::RayReconstruction)
    }

    #[inline]
    pub(super) fn active_feature(self) -> Option<DlssFeature>
    {
        if self.dlss_sr_mode == DlssSrMode::Off {
            None
        } else if self.dlss_rr_enabled {
            Some(DlssFeature::RayReconstruction)
        } else {
            Some(DlssFeature::SuperResolution)
        }
    }
}
