use truvis_render_foundation::render_view::RenderViewAccumSignature;

/// 当前 main view 的 temporal accumulation 状态。
///
/// 它记录上一帧用于累积判断的视图签名，以及当前视图已经连续稳定了多少帧。
/// 这是 runtime 派生状态，不是用户配置；窗口尺寸、view 变化、环境光切换等都会让它 reset。
#[derive(Copy, Clone, Default)]
pub struct ViewAccumState {
    last_render_view: Option<RenderViewAccumSignature>,

    accum_frames_num: usize,
}

impl ViewAccumState {
    /// 根据本帧 view 签名推进累积帧计数。
    ///
    /// 只要相机矩阵或关键视图参数变化，历史图像就不再对应当前 view，计数会回到 0。
    pub fn update_accum_frames(&mut self, render_view: RenderViewAccumSignature) {
        if self.last_render_view != Some(render_view) {
            self.accum_frames_num = 0;
        } else {
            self.accum_frames_num += 1;
        }

        self.last_render_view = Some(render_view);
    }

    /// 清空历史累积状态。
    ///
    /// resize、DLSS render extent 变化、天空贴图切换等都会让旧历史图像失效。
    pub fn reset(&mut self) {
        self.last_render_view = None;
        self.accum_frames_num = 0;
    }

    #[inline]
    pub fn accum_frames_num(&self) -> usize {
        self.accum_frames_num
    }
}
