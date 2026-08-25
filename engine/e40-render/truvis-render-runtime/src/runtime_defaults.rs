use ash::vk;

/// RenderRuntime 自己拥有的默认策略。
///
/// 这些值用于 present 创建、深度格式选择等 runtime 初始化路径，不属于 foundation 公共契约。
pub(crate) struct DefaultRenderRuntimeSettings;

impl DefaultRenderRuntimeSettings {
    pub const DEFAULT_SURFACE_FORMAT: vk::SurfaceFormatKHR = vk::SurfaceFormatKHR {
        // shader 线性输出经过 sRGB swapchain 时会由硬件转换到显示空间。
        format: vk::Format::R8G8B8A8_SRGB,
        // 通知 OS 按 sRGB 非线性空间解释最终呈现结果。
        color_space: vk::ColorSpaceKHR::SRGB_NONLINEAR,
    };
    /// 默认使用 MAILBOX，优先保证交互场景下的低延迟；不可用时由 swapchain helper 选择 fallback。
    pub const DEFAULT_PRESENT_MODE: vk::PresentModeKHR = vk::PresentModeKHR::MAILBOX;
    /// runtime 初始化 depth format 时按此顺序查询设备支持度。
    ///
    /// 这些候选只描述 engine 对深度附件精度的偏好；app 不应该依赖其中某一个格式必然可用。
    pub const DEPTH_FORMAT_CANDIDATES: &'static [vk::Format] = &[
        vk::Format::D32_SFLOAT_S8_UINT,
        vk::Format::D32_SFLOAT,
        vk::Format::D24_UNORM_S8_UINT,
        vk::Format::D16_UNORM_S8_UINT,
        vk::Format::D16_UNORM,
    ];
}
