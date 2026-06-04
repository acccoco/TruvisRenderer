use ash::vk;

use crate::pipeline_settings::FrameSettings;
use crate::render_view::RenderView;

/// DLSS SR 每帧 evaluate 需要的 common constants。
///
/// 这里保持为纯数据结构，不直接依赖 Streamline FFI；具体 pass 负责把它转换成
/// `truvis-streamline-binding` 的 raw ABI 类型。矩阵按 Streamline 契约保存为 row-major。
/// 这些字段描述的是当前 viewport 的 temporal 关系，不拥有任何 GPU resource。
#[derive(Clone, Copy, Debug)]
pub struct DlssSrFrameConstants {
    pub camera_view_to_clip: [f32; 16],
    pub clip_to_camera_view: [f32; 16],
    pub clip_to_prev_clip: [f32; 16],
    pub prev_clip_to_clip: [f32; 16],
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
            camera_view_to_clip: row_major(glam::Mat4::IDENTITY),
            clip_to_camera_view: row_major(glam::Mat4::IDENTITY),
            clip_to_prev_clip: row_major(glam::Mat4::IDENTITY),
            prev_clip_to_clip: row_major(glam::Mat4::IDENTITY),
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

/// DLSS SR 的 temporal 状态。
///
/// 职责边界：
/// - 负责把 app 提供的当前 `RenderView` 转换成 Streamline common constants；
/// - 持有上一帧 `RenderView`，用于计算 current clip 与 previous clip 的转换；
/// - 在 resize、DLSS mode 切换等历史失效点提供 reset 标记。
///
/// 非职责：
/// - 不创建或 tag GPU resource；
/// - 不调用 Streamline API；
/// - 不决定 DLSS mode 或 render extent。
#[derive(Clone, Copy, Debug)]
pub struct DlssSrState {
    constants: DlssSrFrameConstants,
    previous_view: Option<RenderView>,
    reset_pending: bool,
}

impl Default for DlssSrState {
    fn default() -> Self {
        Self {
            constants: DlssSrFrameConstants::default(),
            previous_view: None,
            reset_pending: true,
        }
    }
}

impl DlssSrState {
    #[inline]
    pub fn constants(&self) -> DlssSrFrameConstants {
        self.constants
    }

    /// 请求下一次 evaluate 重置 DLSS history。
    ///
    /// 调用点包括窗口尺寸变化、render extent 变化、DLSS mode 切换和未来场景大跳变。
    pub fn request_reset(&mut self) {
        self.previous_view = None;
        self.reset_pending = true;
        self.constants.reset = true;
    }

    /// 根据当前视图更新 common constants。
    ///
    /// `previous_view` 只用于生成上一帧 clip 空间关系；当 history reset pending 时，
    /// 当前帧仍会写出有效矩阵，但 `reset=true` 会通知 DLSS 丢弃内部历史。
    pub fn update(&mut self, render_view: &RenderView, frame_settings: &FrameSettings) {
        let previous_view = self.previous_view.unwrap_or(*render_view);
        // Streamline 需要 current clip <-> previous clip 的变换来补足 camera motion。
        // 当前 shader motion vectors 第一版写 0，因此这里的矩阵关系是相机运动的主要来源。
        let current_clip_from_world = render_view.projection * render_view.view;
        let previous_clip_from_world = previous_view.projection * previous_view.view;
        let clip_to_prev_clip = previous_clip_from_world * current_clip_from_world.inverse();
        let prev_clip_to_clip = clip_to_prev_clip.inverse();
        let reset = self.reset_pending || self.previous_view.is_none();

        self.constants = DlssSrFrameConstants {
            camera_view_to_clip: row_major(render_view.projection),
            clip_to_camera_view: row_major(render_view.inv_projection),
            clip_to_prev_clip: row_major(clip_to_prev_clip),
            prev_clip_to_clip: row_major(prev_clip_to_clip),
            jitter_offset: [0.0, 0.0],
            // 当前 RT shader 只写物体 motion vector，且第一版为 0；camera motion 交给
            // Streamline 根据矩阵计算，因此 scale 只需保持有效值。
            mvec_scale: [1.0, 1.0],
            camera_pos: render_view.position_ws.to_array(),
            camera_up: normalize_or(render_view.inv_view.y_axis.truncate(), glam::Vec3::Y).to_array(),
            camera_right: normalize_or(render_view.inv_view.x_axis.truncate(), glam::Vec3::X).to_array(),
            camera_fwd: normalize_or(render_view.forward_ws, -glam::Vec3::Z).to_array(),
            camera_near: estimate_camera_near(render_view.projection),
            camera_far: 10000.0,
            camera_fov: estimate_vertical_fov(render_view.projection),
            camera_aspect_ratio: extent_aspect(frame_settings.output_extent),
            motion_vectors_invalid_value: -65504.0,
            depth_inverted: false,
            camera_motion_included: false,
            motion_vectors_3d: false,
            reset,
        };

        self.previous_view = Some(*render_view);
        self.reset_pending = false;
    }
}

fn row_major(matrix: glam::Mat4) -> [f32; 16] {
    // glam 内部按列存储；Streamline common constants 按 row-major 语义读取。
    let cols = matrix.to_cols_array_2d();
    [
        cols[0][0], cols[1][0], cols[2][0], cols[3][0], cols[0][1], cols[1][1], cols[2][1], cols[3][1], cols[0][2],
        cols[1][2], cols[2][2], cols[3][2], cols[0][3], cols[1][3], cols[2][3], cols[3][3],
    ]
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
