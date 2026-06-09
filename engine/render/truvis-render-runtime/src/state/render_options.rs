use crate::state::dlss_sr::DlssSrMode;

/// 用户或调试 UI 可修改的全局渲染选项。
///
/// 这里只保留会影响 runtime 级渲染状态的开关。具体管线自己的 debug channel、legacy
/// denoise 参数或实验性 IC 开关不放在这里，避免 engine 全局状态反向承载 app 语义。
#[derive(Copy, Clone)]
pub struct RenderOptions {
    /// DLSS SR / DLAA 模式。
    ///
    /// 修改后由 `RenderRuntime::sync_render_options_frame_state` 统一解析，必要时会更新
    /// `FrameRenderState`、触发 app-owned target rebuild，并重置 DLSS history。
    pub dlss_sr_mode: DlssSrMode,
    /// DLSS Ray Reconstruction 开关。
    ///
    /// RR 复用 SR 的质量模式与 render/output extent 约定，但执行层会替换 `kFeatureDLSS`
    /// 为 `kFeatureDLSS_RR`。当 SR mode 为 `Off` 时，该开关不会让 RT 主流程进入 RR。
    pub dlss_rr_enabled: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            dlss_sr_mode: DlssSrMode::Off,
            dlss_rr_enabled: false,
        }
    }
}
