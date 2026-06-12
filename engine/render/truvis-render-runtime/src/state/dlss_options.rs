use crate::state::dlss_sr::DlssSrMode;

/// 当前帧最终启用的 DLSS feature。
///
/// SR 与 RR 共享 `DlssSrMode` 的质量档位，但 Streamline 内部资源和 evaluate 入口不同；
/// runtime 用这个枚举表达“需要释放哪个 feature resource / app graph 应进入哪个分支”。
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DlssFeature {
    SuperResolution,
    RayReconstruction,
}

/// 用户或调试 UI 可修改的 DLSS 选项。
///
/// 这个类型同时承担配置输入与 feature 决策职责：它不拥有 GPU resource，也不调用
/// Streamline API，只把 SR mode 与 RR 开关收敛成 runtime / app graph / pass 共用的
/// 单一语义入口。
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct DlssOptions {
    /// DLSS SR / DLAA 模式。
    ///
    /// 修改后由 `RenderRuntime::sync_dlss_options_frame_state` 统一解析，必要时会更新
    /// `FrameRenderState`、触发 app-owned target rebuild，并重置 DLSS history。
    pub dlss_sr_mode: DlssSrMode,
    /// DLSS Ray Reconstruction 开关。
    ///
    /// RR 复用 SR 的质量模式与 render/output extent 约定，但执行层会替换 `kFeatureDLSS`
    /// 为 `kFeatureDLSS_RR`。当 SR mode 为 `Off` 时，该开关不会让 RT 主流程进入 RR。
    pub dlss_rr_enabled: bool,
}

impl Default for DlssOptions {
    fn default() -> Self {
        Self {
            dlss_sr_mode: DlssSrMode::Off,
            dlss_rr_enabled: false,
        }
    }
}

impl DlssOptions {
    /// Native fallback：不调用任何 DLSS evaluate。
    pub const NATIVE: Self = Self {
        dlss_sr_mode: DlssSrMode::Off,
        dlss_rr_enabled: false,
    };

    pub const fn new(dlss_sr_mode: DlssSrMode, dlss_rr_enabled: bool) -> Self {
        Self {
            dlss_sr_mode,
            dlss_rr_enabled,
        }
    }

    #[inline]
    pub fn sr_mode(self) -> DlssSrMode {
        self.dlss_sr_mode
    }

    #[inline]
    pub fn rr_enabled(self) -> bool {
        self.dlss_rr_enabled
    }

    #[inline]
    pub fn is_dlss_active(self) -> bool {
        self.active_feature().is_some()
    }

    #[inline]
    pub fn is_sr_active(self) -> bool {
        self.active_feature() == Some(DlssFeature::SuperResolution)
    }

    #[inline]
    pub fn is_rr_active(self) -> bool {
        self.active_feature() == Some(DlssFeature::RayReconstruction)
    }

    #[inline]
    pub fn active_feature(self) -> Option<DlssFeature> {
        if self.dlss_sr_mode == DlssSrMode::Off {
            None
        } else if self.dlss_rr_enabled {
            Some(DlssFeature::RayReconstruction)
        } else {
            Some(DlssFeature::SuperResolution)
        }
    }
}
