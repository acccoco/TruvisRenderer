use renderer_kit::{camera::Camera, input_state::InputState};

use crate::overlay_geometry::OverlayGeometry;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    fn index(self) -> usize {
        match self {
            Self::X => 0,
            Self::Y => 1,
            Self::Z => 2,
        }
    }

    fn direction(self) -> glam::Vec3 {
        match self {
            Self::X => glam::Vec3::X,
            Self::Y => glam::Vec3::Y,
            Self::Z => glam::Vec3::Z,
        }
    }

    fn all() -> [Self; 3] {
        [Self::X, Self::Y, Self::Z]
    }
}

/// 同一条裁剪后的屏幕轴线同时决定绘制与命中。
#[derive(Clone, Copy)]
struct ProjectedAxis {
    start: glam::Vec2,
    end: glam::Vec2,
    draw_head: bool,
}

struct TransformGizmoFrame {
    origin_ws: glam::Vec3,
    projected_axes: [Option<ProjectedAxis>; 3],
    viewport: glam::Vec2,
}

impl TransformGizmoFrame {
    const AXIS_LENGTH_PX: f32 = 96.0;
    const PICK_RADIUS_PX: f32 = 10.0;
    const MIN_AXIS_LENGTH_PX: f32 = 8.0;
    const SHAFT_WIDTH_PX: f32 = 4.0;
    const HEAD_WIDTH_PX: f32 = 14.0;
    const HEAD_LENGTH_PX: f32 = 16.0;

    fn new(transform: glam::Mat4, camera: &Camera, viewport: glam::Vec2) -> Option<Self> {
        let origin_ws = transform.w_axis.truncate();
        camera.project_to_viewport(origin_ws, viewport)?;
        let distance = (origin_ws - camera.position).length().max(0.25);
        let visible_height = 2.0 * distance * (camera.fov_deg_vertical.to_radians() * 0.5).tan();
        let axis_length_ws = visible_height * Self::AXIS_LENGTH_PX / viewport.y;
        if !axis_length_ws.is_finite() || axis_length_ws <= 0.0 {
            return None;
        }
        let projected_axes = Axis::all().map(|axis| {
            let endpoint = origin_ws + axis.direction() * axis_length_ws;
            let (start, end) = camera.project_segment_to_viewport(origin_ws, endpoint, viewport)?;
            let length = start.distance(end);
            if !length.is_finite() || length < Self::MIN_AXIS_LENGTH_PX {
                return None;
            }
            Some(ProjectedAxis {
                start,
                end,
                draw_head: camera.project_to_viewport(endpoint, viewport).is_some(),
            })
        });
        Some(Self {
            origin_ws,
            projected_axes,
            viewport,
        })
    }

    fn hit_axis(&self, mouse: glam::Vec2) -> Option<Axis> {
        if !mouse.is_finite() || mouse.min_element() < 0.0 || mouse.x >= self.viewport.x || mouse.y >= self.viewport.y {
            return None;
        }
        Axis::all()
            .into_iter()
            .filter_map(|axis| {
                let projected = self.projected_axes[axis.index()]?;
                let segment = projected.end - projected.start;
                let t = ((mouse - projected.start).dot(segment) / segment.length_squared()).clamp(0.0, 1.0);
                let distance = mouse.distance_squared(projected.start + segment * t);
                (distance <= Self::PICK_RADIUS_PX * Self::PICK_RADIUS_PX).then_some((axis, distance))
            })
            .min_by(|(a, da), (b, db)| da.total_cmp(db).then_with(|| b.index().cmp(&a.index())))
            .map(|(axis, _)| axis)
    }

    fn axis_parameter(&self, axis: Axis, camera: &Camera, screen_pos: glam::Vec2, viewport: glam::Vec2) -> Option<f32> {
        self.axis_parameter_from_pivot(self.origin_ws, axis, camera, screen_pos, viewport)
    }

    fn axis_parameter_from_pivot(
        &self,
        pivot_ws: glam::Vec3,
        axis: Axis,
        camera: &Camera,
        screen_pos: glam::Vec2,
        viewport: glam::Vec2,
    ) -> Option<f32> {
        let direction = camera.screen_ray_direction(screen_pos, viewport)?;
        let axis_ws = axis.direction();
        let normal = Self::drag_plane_normal(axis_ws, camera)?;
        let denominator = direction.dot(normal);
        if denominator.abs() <= 0.00001 {
            return None;
        }
        let distance = (pivot_ws - camera.position).dot(normal) / denominator;
        if !distance.is_finite() {
            return None;
        }
        let intersection = camera.position + direction * distance;
        let parameter = (intersection - pivot_ws).dot(axis_ws);
        parameter.is_finite().then_some(parameter)
    }

    fn drag_plane_normal(axis_ws: glam::Vec3, camera: &Camera) -> Option<glam::Vec3> {
        let mut normal = camera.camera_forward() - axis_ws * camera.camera_forward().dot(axis_ws);
        if normal.length_squared() <= 0.00001 {
            normal = camera.camera_right() - axis_ws * camera.camera_right().dot(axis_ws);
        }
        if normal.length_squared() <= 0.00001 || !normal.is_finite() {
            return None;
        }
        Some(normal.normalize())
    }
}

struct AxisDragState {
    axis: Axis,
    initial_transform: glam::Mat4,
    pivot_ws: glam::Vec3,
    start_parameter: f32,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct TransformGizmoUpdate {
    pub transform: Option<glam::Mat4>,
    pub drag_started: bool,
    pub drag_finished: bool,
    pub consumed: bool,
}

/// 只计算交互与本帧 CPU 展示；对象身份和 mutation 属于 Renderer。
#[derive(Default)]
pub(crate) struct TransformGizmo {
    frame: Option<TransformGizmoFrame>,
    hovered_axis: Option<Axis>,
    drag: Option<AxisDragState>,
}

impl TransformGizmo {
    pub(crate) fn update(
        &mut self,
        target: Option<glam::Mat4>,
        camera: &Camera,
        input: &InputState,
        viewport: glam::Vec2,
    ) -> TransformGizmoUpdate {
        self.refresh_frame(target, camera, viewport);
        if self.frame.is_none() {
            let was_dragging = self.cancel_drag();
            return TransformGizmoUpdate {
                drag_finished: was_dragging,
                consumed: was_dragging,
                ..Default::default()
            };
        }
        let mouse = input.mouse_position();
        let mouse = glam::vec2(mouse[0] as f32, mouse[1] as f32);
        if self.drag.is_some() {
            return self.update_drag(camera, input, viewport, mouse);
        }
        if input.is_left_button_just_pressed() {
            let press = input.left_button_press_position;
            let press = glam::vec2(press[0] as f32, press[1] as f32);
            let frame = self.frame.as_ref().unwrap();
            if let Some(axis) = frame.hit_axis(press) {
                if let Some(start_parameter) = frame.axis_parameter(axis, camera, press, viewport) {
                    self.drag = Some(AxisDragState {
                        axis,
                        initial_transform: target.unwrap(),
                        pivot_ws: frame.origin_ws,
                        start_parameter,
                    });
                    let mut update = self.update_drag(camera, input, viewport, mouse);
                    update.drag_started = true;
                    return update;
                }
                // 可见轴已命中，即使无法建立拖动平面，也不能穿透选择背景。
                return TransformGizmoUpdate {
                    consumed: true,
                    ..Default::default()
                };
            }
        }
        TransformGizmoUpdate::default()
    }

    /// 松开帧仍提交最终位移；展示只在 Renderer 成功写回 World 后刷新。
    fn update_drag(
        &mut self,
        camera: &Camera,
        input: &InputState,
        viewport: glam::Vec2,
        mouse: glam::Vec2,
    ) -> TransformGizmoUpdate {
        let transform = self.drag.as_ref().and_then(|drag| {
            let parameter =
                self.frame.as_ref()?.axis_parameter_from_pivot(drag.pivot_ws, drag.axis, camera, mouse, viewport)?;
            let transform = glam::Mat4::from_translation(drag.axis.direction() * (parameter - drag.start_parameter))
                * drag.initial_transform;
            transform.is_finite().then_some(transform)
        });
        let finished = input.is_left_button_just_released() || !input.is_left_button_pressed();
        if finished {
            self.drag = None;
        }
        TransformGizmoUpdate {
            transform,
            drag_finished: finished,
            consumed: true,
            ..Default::default()
        }
    }

    pub(crate) fn refresh_frame(&mut self, target: Option<glam::Mat4>, camera: &Camera, viewport: glam::Vec2) {
        self.frame = target
            .filter(|value| value.is_finite())
            .and_then(|value| TransformGizmoFrame::new(value, camera, viewport));
    }

    /// 返回 gizmo 是否独占 hover，灯光不再同时高亮。
    pub(crate) fn update_hover(&mut self, mouse: Option<glam::Vec2>) -> bool {
        self.hovered_axis = mouse.and_then(|mouse| self.frame.as_ref()?.hit_axis(mouse));
        self.drag.is_some() || self.hovered_axis.is_some()
    }

    pub(crate) fn cancel_drag(&mut self) -> bool {
        let was_dragging = self.drag.take().is_some();
        self.frame = None;
        self.hovered_axis = None;
        was_dragging
    }

    pub(crate) fn append_geometry(&self, geometry: &mut OverlayGeometry<'_>) {
        let Some(frame) = self.frame.as_ref() else {
            return;
        };
        let active_axis = self.drag.as_ref().map(|drag| drag.axis);
        for axis in Axis::all() {
            let Some(projected) = frame.projected_axes[axis.index()] else {
                continue;
            };
            let mut color = OverlayGeometry::AXIS_COLORS[axis.index()];
            if let Some(active) = active_axis {
                if active == axis {
                    color = (color.truncate() * 1.25).min(glam::Vec3::ONE).extend(1.0);
                } else {
                    color = (color.truncate() * 0.45).extend(0.72);
                }
            } else if self.hovered_axis == Some(axis) {
                color = (color.truncate() * 1.25).min(glam::Vec3::ONE).extend(1.0);
            }
            if projected.draw_head {
                let head_length =
                    TransformGizmoFrame::HEAD_LENGTH_PX.min(projected.start.distance(projected.end) * 0.4);
                geometry.arrow(
                    projected.start,
                    projected.end,
                    TransformGizmoFrame::SHAFT_WIDTH_PX,
                    TransformGizmoFrame::HEAD_WIDTH_PX,
                    head_length,
                    color,
                );
            } else {
                geometry.line(projected.start, projected.end, TransformGizmoFrame::SHAFT_WIDTH_PX, color);
            }
        }
    }
}
