use ash::vk;

/// 当前 main view / frame 的渲染目标状态。
///
/// 这是运行时根据窗口、present、DLSS SR mode 和设备能力推导出的状态，不是用户直接配置。
/// App-owned RT target、GBuffer、DLSS input/output 和 main-view target 都以它作为尺寸与格式契约。
#[derive(Copy, Clone, Default, PartialEq, Eq)]
pub struct FrameRenderState {
    /// app 层 HDR 中间图像使用的颜色格式；不同于 swapchain surface format。
    pub hdr_color_format: vk::Format,
    /// app 层 depth attachment 使用的格式，由 runtime 按设备能力选择。
    pub depth_format: vk::Format,
    /// RT、GBuffer、motion vector 等内部渲染资源的尺寸。
    ///
    /// DLSS SR upscale mode 下通常小于 `output_extent`；native / DLAA / fallback 路径下与
    /// `output_extent` 相同。
    pub render_extent: vk::Extent2D,
    /// present、GUI 和最终 main-view color 的输出尺寸，通常等于 swapchain extent。
    pub output_extent: vk::Extent2D,
}

impl FrameRenderState {
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
