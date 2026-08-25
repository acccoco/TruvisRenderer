use ash::vk;
use truvis_render_foundation::render_view::RenderView;
use truvis_streamline_binding::dlss;

use crate::state::frame_state::FrameRenderState;

/// DLSS Super Resolution / DLAA 模式。
///
/// 这里只表示 `kFeatureDLSS` 的模式选择；Ray Reconstruction 后续作为独立开关，
/// 在执行层替换 SR evaluate，而不是作为这里的另一个互斥质量模式。
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum DlssSrMode {
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

    /// 转换为 Streamline 的 SR/DLAA quality mode。
    ///
    /// 这里是项目内唯一的 `DlssSrMode -> dlss::DlssMode` 映射入口；RR 仍复用同一
    /// quality mode，但是否走 `kFeatureDLSS_RR` 由 `DlssOptions` 决定。
    pub fn to_streamline_mode(self) -> dlss::DlssMode {
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

/// DLSS SR 每帧 evaluate 需要的 common constants。
///
/// 这里保持为纯数据结构，不直接依赖 Streamline FFI；具体 pass 负责把它转换成
/// `truvis-streamline-binding` 的 raw ABI 类型。矩阵按 Streamline 契约保存为 row-major。
/// 这些字段描述的是当前 viewport 的 temporal 关系，不拥有任何 GPU resource。
#[derive(Clone, Copy, Debug)]
pub struct DlssSrFrameConstants {
    pub camera_view_to_clip: [f32; 16],
    pub clip_to_camera_view: [f32; 16],
    pub world_to_camera_view: [f32; 16],
    pub camera_view_to_world: [f32; 16],
    pub clip_to_prev_clip: [f32; 16],
    pub prev_clip_to_clip: [f32; 16],
    /// shader 侧使用的当前帧采样偏移，单位为 render target pixel。
    ///
    /// 方向语义是从 unjittered 像素中心偏移到本帧实际 primary ray 采样点，只允许写入
    /// `PerFrameData::temporal_jitter_px`。它不是 Streamline common constants 的
    /// `jitterOffset`，不要直接传给 Streamline。
    pub sampling_jitter_offset: [f32; 2],
    /// Streamline `sl::Constants::jitterOffset`，单位为 render target pixel。
    ///
    /// 方向语义是从本帧 jittered 输入回正到 unjittered 像素中心，因此必须和
    /// `sampling_jitter_offset` 反号。该契约与 `motionVectorsJittered = false` 配套：
    /// motion vector 按无 jitter 投影计算，jitter delta 只通过这里单独交给 Streamline。
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

impl Default for DlssSrFrameConstants {
    fn default() -> Self {
        Self {
            camera_view_to_clip: Self::row_major(glam::Mat4::IDENTITY),
            clip_to_camera_view: Self::row_major(glam::Mat4::IDENTITY),
            world_to_camera_view: Self::row_major(glam::Mat4::IDENTITY),
            camera_view_to_world: Self::row_major(glam::Mat4::IDENTITY),
            clip_to_prev_clip: Self::row_major(glam::Mat4::IDENTITY),
            prev_clip_to_clip: Self::row_major(glam::Mat4::IDENTITY),
            sampling_jitter_offset: [0.0, 0.0],
            jitter_offset: [0.0, 0.0],
            mvec_scale: [1.0, 1.0],
            camera_pos: [0.0, 0.0, 0.0],
            camera_up: [0.0, 1.0, 0.0],
            camera_right: [1.0, 0.0, 0.0],
            camera_fwd: [0.0, 0.0, -1.0],
            camera_near: 0.1,
            camera_far: 10000.0,
            camera_fov: 60.0_f32.to_radians(),
            camera_aspect_ratio: 1.0,
            motion_vectors_invalid_value: -65504.0,
            depth_inverted: false,
            camera_motion_included: false,
            motion_vectors_3d: false,
            reset: true,
        }
    }
}

impl DlssSrFrameConstants {
    fn row_major(matrix: glam::Mat4) -> [f32; 16] {
        // glam 内部按列存储；Streamline common constants 按 row-major 语义读取。
        let cols = matrix.to_cols_array_2d();
        [
            cols[0][0], cols[1][0], cols[2][0], cols[3][0], cols[0][1], cols[1][1], cols[2][1], cols[3][1], cols[0][2],
            cols[1][2], cols[2][2], cols[3][2], cols[0][3], cols[1][3], cols[2][3], cols[3][3],
        ]
    }
}

/// DLSS temporal jitter 的短生命周期状态。
///
/// sequence index 必须跟随 `DlssSrState` reset 一起回到固定起点；DLSS 关闭时不推进
/// 序列，避免 native 路径隐藏消耗采样状态。
#[derive(Clone, Copy, Debug, Default)]
struct DlssJitterSequence {
    index: u32,
}

impl DlssJitterSequence {
    fn reset(&mut self) {
        self.index = 0;
    }

    fn next_offset(&mut self, dlss_active: bool) -> [f32; 2] {
        if !dlss_active {
            return [0.0, 0.0];
        }

        let next_index = self.index.wrapping_add(1);
        let index = if next_index == 0 { 1 } else { next_index };
        self.index = index;
        [Self::halton(index, 2) - 0.5, Self::halton(index, 3) - 0.5]
    }

    fn halton(mut index: u32, base: u32) -> f32 {
        debug_assert!(base > 1);

        let mut fraction = 1.0;
        let mut result = 0.0;
        while index > 0 {
            fraction /= base as f32;
            result += fraction * (index % base) as f32;
            index /= base;
        }
        result
    }
}

/// 将 `RenderView` 与当前 frame state 收敛为 Streamline common constants。
///
/// builder 不持有跨帧状态，也不接触 Streamline FFI；它只负责一次 `update` 内的矩阵、
/// camera 和 extent 派生，保证 `DlssSrState` 聚焦 temporal 状态推进。
#[derive(Clone, Copy)]
struct DlssCommonConstantsBuilder<'a> {
    render_view: &'a RenderView,
    previous_view: RenderView,
    frame_state: &'a FrameRenderState,
    jitter_offset: [f32; 2],
    reset: bool,
}

impl<'a> DlssCommonConstantsBuilder<'a> {
    fn new(
        render_view: &'a RenderView,
        previous_view: RenderView,
        frame_state: &'a FrameRenderState,
        jitter_offset: [f32; 2],
        reset: bool,
    ) -> Self {
        Self {
            render_view,
            previous_view,
            frame_state,
            jitter_offset,
            reset,
        }
    }

    fn build(self) -> DlssSrFrameConstants {
        let current_clip_from_world = self.render_view.projection * self.render_view.view;
        let previous_clip_from_world = self.previous_view.projection * self.previous_view.view;
        // Streamline 仍需要 current clip <-> previous clip 的关系；shader 写入的 2D
        // motion vector 已包含 camera motion，因此这里不再让 Streamline 补相机运动。
        let clip_to_prev_clip = previous_clip_from_world * current_clip_from_world.inverse();
        let prev_clip_to_clip = clip_to_prev_clip.inverse();

        DlssSrFrameConstants {
            camera_view_to_clip: DlssSrFrameConstants::row_major(self.render_view.projection),
            clip_to_camera_view: DlssSrFrameConstants::row_major(self.render_view.inv_projection),
            world_to_camera_view: DlssSrFrameConstants::row_major(self.render_view.view),
            camera_view_to_world: DlssSrFrameConstants::row_major(self.render_view.inv_view),
            clip_to_prev_clip: DlssSrFrameConstants::row_major(clip_to_prev_clip),
            prev_clip_to_clip: DlssSrFrameConstants::row_major(prev_clip_to_clip),
            sampling_jitter_offset: self.jitter_offset,
            // shader 通过 pixel_center += sampling_jitter_offset 让当前帧输入产生亚像素采样；
            // Streamline common constants 的 jitterOffset 描述的是当前输入相对 unjittered
            // 像素中心的回正偏移。两者必须反号，否则 RR 会把静止画面的采样 jitter 当作
            // 真实位移，低 render extent 下会被 upscale ratio 放大成天空和轮廓抖动。
            jitter_offset: [-self.jitter_offset[0], -self.jitter_offset[1]],
            // shader 写入 pixel-space motion vector，方向为当前像素回溯到上一帧位置。
            // Streamline 通过 scale 归一化到 [-1, 1]。
            mvec_scale: [
                Self::reciprocal_extent(self.frame_state.render_extent.width),
                Self::reciprocal_extent(self.frame_state.render_extent.height),
            ],
            camera_pos: self.render_view.position_ws.to_array(),
            camera_up: Self::normalize_or(self.render_view.inv_view.y_axis.truncate(), glam::Vec3::Y).to_array(),
            camera_right: Self::normalize_or(self.render_view.inv_view.x_axis.truncate(), glam::Vec3::X).to_array(),
            camera_fwd: Self::normalize_or(self.render_view.forward_ws, -glam::Vec3::Z).to_array(),
            camera_near: Self::estimate_camera_near(self.render_view.projection),
            camera_far: 10000.0,
            camera_fov: Self::estimate_vertical_fov(self.render_view.projection),
            camera_aspect_ratio: Self::extent_aspect(self.frame_state.output_extent),
            motion_vectors_invalid_value: -65504.0,
            depth_inverted: false,
            camera_motion_included: true,
            motion_vectors_3d: false,
            reset: self.reset,
        }
    }

    fn reciprocal_extent(value: u32) -> f32 {
        if value == 0 { 1.0 } else { 1.0 / value as f32 }
    }

    fn normalize_or(value: glam::Vec3, fallback: glam::Vec3) -> glam::Vec3 {
        value.try_normalize().unwrap_or(fallback)
    }

    fn estimate_vertical_fov(projection: glam::Mat4) -> f32 {
        let cot_half_fov = projection.y_axis.y.abs();
        if cot_half_fov > f32::EPSILON { 2.0 * (1.0 / cot_half_fov).atan() } else { 60.0_f32.to_radians() }
    }

    fn estimate_camera_near(projection: glam::Mat4) -> f32 {
        // 当前 RenderView 没有直接保留 near/far；这里给 Streamline 提供一个稳定的近似值。
        // 后续如果相机系统显式保存 near/far，应替换为真实相机参数。
        let near = -projection.w_axis.z * 0.5;
        if near.is_finite() && near > 0.0 { near } else { 0.1 }
    }

    fn extent_aspect(extent: vk::Extent2D) -> f32 {
        if extent.height == 0 { 1.0 } else { extent.width as f32 / extent.height as f32 }
    }
}

/// DLSS SR 的 temporal 状态。
///
/// 职责边界：
/// - 负责把 app 提供的当前 `RenderView` 转换成 Streamline common constants；
/// - 持有上一帧 `RenderView`，用于计算 current clip 与 previous clip 的转换；
/// - 维护 DLSS 使用的帧级 temporal jitter，并在 resize、DLSS mode 切换等历史失效点
///   提供 reset 标记。
///
/// 非职责：
/// - 不创建或 tag GPU resource；
/// - 不调用 Streamline API；
/// - 不决定 DLSS mode 或 render extent。
#[derive(Clone, Copy, Debug)]
pub struct DlssSrState {
    constants: DlssSrFrameConstants,
    previous_view: Option<RenderView>,
    motion_vector_previous_view: Option<RenderView>,
    jitter_sequence: DlssJitterSequence,
    reset_pending: bool,
}

impl Default for DlssSrState {
    fn default() -> Self {
        Self {
            constants: DlssSrFrameConstants::default(),
            previous_view: None,
            motion_vector_previous_view: None,
            jitter_sequence: DlssJitterSequence::default(),
            reset_pending: true,
        }
    }
}

impl DlssSrState {
    #[inline]
    pub fn constants(&self) -> DlssSrFrameConstants {
        self.constants
    }

    #[inline]
    pub fn motion_vector_previous_view(&self) -> Option<RenderView> {
        self.motion_vector_previous_view
    }

    /// 请求下一次 evaluate 重置 DLSS history。
    ///
    /// 调用点包括窗口尺寸变化、render extent 变化、DLSS mode 切换和未来场景大跳变。
    pub fn request_reset(&mut self) {
        self.previous_view = None;
        self.motion_vector_previous_view = None;
        self.jitter_sequence.reset();
        self.reset_pending = true;
        self.constants.sampling_jitter_offset = [0.0, 0.0];
        self.constants.jitter_offset = [0.0, 0.0];
        self.constants.reset = true;
    }

    /// 根据当前视图更新 common constants。
    ///
    /// `previous_view` 只用于生成上一帧 clip 空间关系；当 history reset pending 时，
    /// 当前帧仍会写出有效矩阵，但 `reset=true` 会通知 DLSS 丢弃内部历史。
    ///
    /// `dlss_active` 决定本帧是否生成 temporal jitter。DLSS 关闭时必须写 0 且不推进
    /// jitter sequence，避免 native 路径和下一次 DLSS reset 继承不可见的采样状态。
    pub fn update(&mut self, render_view: &RenderView, frame_state: &FrameRenderState, dlss_active: bool) {
        let previous_view = self.previous_view.unwrap_or(*render_view);
        let reset = self.reset_pending || self.previous_view.is_none();
        let jitter_offset = self.jitter_sequence.next_offset(dlss_active);
        self.constants =
            DlssCommonConstantsBuilder::new(render_view, previous_view, frame_state, jitter_offset, reset).build();

        self.motion_vector_previous_view = Some(previous_view);
        self.previous_view = Some(*render_view);
        self.reset_pending = false;
    }
}
