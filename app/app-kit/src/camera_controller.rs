use truvis_app_frame::input_event::KeyCode;
use truvis_render_runtime::platform::camera::Camera;
use truvis_render_runtime::ray_cast::{RayCastRay, RayCastResult};

use crate::input_state::InputState;

pub struct CameraController {
    camera: Camera,
    pending_pivot_raycast: Option<PivotRayCastRequest>,
    active_pivot_orbit: Option<PivotOrbitState>,
    pending_drag_pan_raycast: Option<DragPanRayCastRequest>,
    active_drag_pan: Option<DragPanState>,
    active_middle_button_mode: Option<MiddleButtonMode>,
    pending_wheel_zoom_raycast: Option<WheelZoomRayCastRequest>,
    active_wheel_zoom: Option<WheelZoomState>,
}

/// 中键按下后交给 App 在 after_prepare 阶段执行的 pivot 查询。
#[derive(Clone, Copy, Debug)]
pub struct PivotRayCastRequest {
    pub ray: RayCastRay,
    anchor_screen_pos: glam::Vec2,
}

/// Shift + 中键按下后交给 App 在 after_prepare 阶段执行的拖拽锚点查询。
#[derive(Clone, Copy, Debug)]
pub struct DragPanRayCastRequest {
    pub ray: RayCastRay,
}

/// 滚轮连续缩放开始时交给 App 在 after_prepare 阶段执行的锚点查询。
///
/// 请求保存首个滚轮事件所在的屏幕锚点和 fallback 锚点。App 只负责执行 `ray`，
/// 命中、未命中和累计滚轮量的解释都留在控制器内部，避免把相机交互策略泄漏到具体 App。
#[derive(Clone, Copy, Debug)]
pub struct WheelZoomRayCastRequest {
    pub ray: RayCastRay,
    anchor_screen_pos: glam::Vec2,
    viewport_size: glam::Vec2,
    fallback_anchor_ws: glam::Vec3,
    pending_wheel_delta: f32,
}

struct PivotOrbitState {
    pivot_ws: glam::Vec3,
    anchor_screen_pos: glam::Vec2,
    distance: f32,
}

struct DragPanState {
    anchor_ws: glam::Vec3,
    distance: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MiddleButtonMode {
    PivotOrbit,
    DragPan,
}

struct WheelZoomState {
    anchor_ws: glam::Vec3,
    anchor_screen_pos: glam::Vec2,
    distance: f32,
    idle_time_s: f32,
}

impl Default for CameraController {
    fn default() -> Self {
        Self {
            camera: Camera::default(),
            pending_pivot_raycast: None,
            active_pivot_orbit: None,
            pending_drag_pan_raycast: None,
            active_drag_pan: None,
            active_middle_button_mode: None,
            pending_wheel_zoom_raycast: None,
            active_wheel_zoom: None,
        }
    }
}

impl CameraController {
    const ROTATE_SENSITIVITY_DIVISOR: f32 = 7.0;
    const MOVE_SPEED: f32 = 320.0;
    const SCREEN_RAY_T_MAX: f32 = 10000.0;
    const WHEEL_ZOOM_IDLE_RESET_S: f32 = 0.15;
    const WHEEL_ZOOM_DISTANCE_SCALE_PER_DELTA: f32 = 0.15;
    const WHEEL_ZOOM_EXP_LIMIT: f32 = 4.0;
    const WHEEL_ZOOM_FALLBACK_DISTANCE: f32 = 320.0;

    pub fn camera(&self) -> &Camera {
        &self.camera
    }

    pub fn camera_mut(&mut self) -> &mut Camera {
        &mut self.camera
    }

    pub fn take_pending_pivot_raycast(&mut self) -> Option<PivotRayCastRequest> {
        self.pending_pivot_raycast.take()
    }

    pub fn take_pending_drag_pan_raycast(&mut self) -> Option<DragPanRayCastRequest> {
        self.pending_drag_pan_raycast.take()
    }

    pub fn take_pending_wheel_zoom_raycast(&mut self) -> Option<WheelZoomRayCastRequest> {
        self.pending_wheel_zoom_raycast.take()
    }

    pub fn finish_pivot_raycast(&mut self, request: PivotRayCastRequest, result: Result<RayCastResult, String>) {
        match result {
            Ok(RayCastResult::Hit(hit)) => {
                let distance = (hit.position_ws - request.ray.origin_ws).length();
                if hit.position_ws.is_finite() && distance.is_finite() && distance > self.camera.near.max(0.001) {
                    self.active_pivot_orbit = Some(PivotOrbitState {
                        pivot_ws: hit.position_ws,
                        anchor_screen_pos: request.anchor_screen_pos,
                        distance,
                    });
                } else {
                    self.active_pivot_orbit = None;
                }
            }
            Ok(RayCastResult::Miss) => {
                self.active_pivot_orbit = None;
            }
            Err(err) => {
                log::warn!("pivot orbit raycast failed: {err}");
                self.active_pivot_orbit = None;
            }
        }
    }

    pub fn finish_drag_pan_raycast(&mut self, request: DragPanRayCastRequest, result: Result<RayCastResult, String>) {
        match result {
            Ok(RayCastResult::Hit(hit)) => {
                let distance = (hit.position_ws - request.ray.origin_ws).length();
                if hit.position_ws.is_finite() && distance.is_finite() && distance > self.min_camera_distance() {
                    self.active_drag_pan = Some(DragPanState {
                        anchor_ws: hit.position_ws,
                        distance,
                    });
                } else {
                    self.active_drag_pan = None;
                }
            }
            Ok(RayCastResult::Miss) => {
                self.active_drag_pan = None;
            }
            Err(err) => {
                log::warn!("drag pan raycast failed: {err}");
                self.active_drag_pan = None;
            }
        }
    }

    pub fn finish_wheel_zoom_raycast(
        &mut self,
        request: WheelZoomRayCastRequest,
        result: Result<RayCastResult, String>,
    ) {
        let anchor_ws = match result {
            Ok(RayCastResult::Hit(hit)) if hit.position_ws.is_finite() => hit.position_ws,
            Ok(RayCastResult::Hit(_)) | Ok(RayCastResult::Miss) => request.fallback_anchor_ws,
            Err(err) => {
                log::warn!("wheel zoom raycast failed: {err}");
                request.fallback_anchor_ws
            }
        };

        let distance = (anchor_ws - request.ray.origin_ws).length();
        if !anchor_ws.is_finite() || !distance.is_finite() || distance <= self.min_camera_distance() {
            self.active_wheel_zoom = None;
            return;
        }

        self.active_wheel_zoom = Some(WheelZoomState {
            anchor_ws,
            anchor_screen_pos: request.anchor_screen_pos,
            distance,
            idle_time_s: 0.0,
        });
        self.apply_wheel_zoom_delta(request.pending_wheel_delta, request.viewport_size);
    }

    /// 根据窗口物理像素坐标生成世界空间射线，供 app 层在 after_prepare 阶段执行同步查询。
    pub fn make_screen_raycast(&self, mouse_position: [f64; 2], viewport_size: glam::Vec2) -> Option<RayCastRay> {
        let screen_pos = glam::vec2(mouse_position[0] as f32, mouse_position[1] as f32);
        let direction_ws = Self::screen_ray_direction(&self.camera, screen_pos, viewport_size)?;
        Some(RayCastRay {
            origin_ws: self.camera.position,
            direction_ws,
            t_min: self.camera.near.max(0.001),
            t_max: Self::SCREEN_RAY_T_MAX,
        })
    }

    pub fn update(&mut self, input_state: &InputState, viewport_size: glam::Vec2, delta_time: std::time::Duration) {
        self.update_impl(input_state, viewport_size, delta_time, false);
    }

    /// 更新相机控制，并允许 Shift+中键拖拽和滚轮通过 after_prepare 阶段的 raycast 建立屏幕锚点。
    ///
    /// 未接入拖拽/滚轮 pending raycast 的 App 应继续调用 [`CameraController::update`]，
    /// 避免产生无人消费的同步查询请求。
    pub fn update_with_wheel_zoom(
        &mut self,
        input_state: &InputState,
        viewport_size: glam::Vec2,
        delta_time: std::time::Duration,
    ) {
        self.update_impl(input_state, viewport_size, delta_time, true);
    }

    fn update_impl(
        &mut self,
        input_state: &InputState,
        viewport_size: glam::Vec2,
        delta_time: std::time::Duration,
        raycast_camera_controls_enabled: bool,
    ) {
        let delta_time_s = delta_time.as_secs_f32();

        self.camera.set_aspect_ratio(viewport_size.x / viewport_size.y);

        if !input_state.is_middle_button_pressed() {
            self.clear_middle_button_control();
        } else {
            self.update_middle_button_control(input_state, viewport_size, raycast_camera_controls_enabled);
            self.clear_wheel_zoom();
            return;
        }

        let manual_camera_changed = self.update_manual_camera(input_state, delta_time_s);
        if raycast_camera_controls_enabled {
            if manual_camera_changed {
                self.clear_wheel_zoom();
            }
            self.update_wheel_zoom(input_state, viewport_size, delta_time_s);
        } else {
            self.clear_wheel_zoom();
        }
    }

    fn update_manual_camera(&mut self, input_state: &InputState, delta_time_s: f32) -> bool {
        let mut changed = false;

        if input_state.is_right_button_pressed() {
            let mouse_delta = input_state.get_mouse_delta();
            if mouse_delta[0] != 0.0 || mouse_delta[1] != 0.0 {
                self.rotate_camera(mouse_delta);
                changed = true;
            }
        }

        if input_state.is_key_pressed(KeyCode::KeyW) {
            self.camera.move_forward(delta_time_s * Self::MOVE_SPEED);
            changed = true;
        }
        if input_state.is_key_pressed(KeyCode::KeyS) {
            self.camera.move_forward(-delta_time_s * Self::MOVE_SPEED);
            changed = true;
        }
        if input_state.is_key_pressed(KeyCode::KeyA) {
            self.camera.move_right(-delta_time_s * Self::MOVE_SPEED);
            changed = true;
        }
        if input_state.is_key_pressed(KeyCode::KeyD) {
            self.camera.move_right(delta_time_s * Self::MOVE_SPEED);
            changed = true;
        }
        if input_state.is_key_pressed(KeyCode::KeyE) {
            self.camera.move_up(delta_time_s * Self::MOVE_SPEED);
            changed = true;
        }
        if input_state.is_key_pressed(KeyCode::KeyQ) {
            self.camera.move_up(-delta_time_s * Self::MOVE_SPEED);
            changed = true;
        }

        changed
    }

    fn make_pivot_raycast(&self, mouse_position: [f64; 2], viewport_size: glam::Vec2) -> Option<PivotRayCastRequest> {
        let anchor_screen_pos = glam::vec2(mouse_position[0] as f32, mouse_position[1] as f32);
        let ray = self.make_screen_raycast(mouse_position, viewport_size)?;
        Some(PivotRayCastRequest { ray, anchor_screen_pos })
    }

    fn make_drag_pan_raycast(
        &self,
        mouse_position: [f64; 2],
        viewport_size: glam::Vec2,
    ) -> Option<DragPanRayCastRequest> {
        let ray = self.make_screen_raycast(mouse_position, viewport_size)?;
        Some(DragPanRayCastRequest { ray })
    }

    fn make_wheel_zoom_raycast(
        &self,
        mouse_position: [f64; 2],
        viewport_size: glam::Vec2,
        pending_wheel_delta: f32,
    ) -> Option<WheelZoomRayCastRequest> {
        let anchor_screen_pos = glam::vec2(mouse_position[0] as f32, mouse_position[1] as f32);
        let direction_ws = Self::screen_ray_direction(&self.camera, anchor_screen_pos, viewport_size)?;
        let ray = RayCastRay {
            origin_ws: self.camera.position,
            direction_ws,
            t_min: self.camera.near.max(0.001),
            t_max: Self::SCREEN_RAY_T_MAX,
        };
        let fallback_anchor_ws = self.camera.position + direction_ws * Self::WHEEL_ZOOM_FALLBACK_DISTANCE;
        Some(WheelZoomRayCastRequest {
            ray,
            anchor_screen_pos,
            viewport_size,
            fallback_anchor_ws,
            pending_wheel_delta,
        })
    }

    fn update_middle_button_control(
        &mut self,
        input_state: &InputState,
        viewport_size: glam::Vec2,
        drag_pan_enabled: bool,
    ) {
        let desired_mode = if drag_pan_enabled && input_state.is_shift_pressed() {
            MiddleButtonMode::DragPan
        } else {
            MiddleButtonMode::PivotOrbit
        };

        if self.active_middle_button_mode != Some(desired_mode) {
            self.clear_pivot_orbit();
            self.clear_drag_pan();
            self.active_middle_button_mode = Some(desired_mode);

            match desired_mode {
                MiddleButtonMode::PivotOrbit => {
                    self.pending_pivot_raycast = self.make_pivot_raycast(input_state.mouse_position(), viewport_size);
                }
                MiddleButtonMode::DragPan => {
                    self.pending_drag_pan_raycast =
                        self.make_drag_pan_raycast(input_state.mouse_position(), viewport_size);
                }
            }
        }

        match desired_mode {
            MiddleButtonMode::PivotOrbit => {
                if self.active_pivot_orbit.is_some() {
                    self.update_pivot_orbit(input_state, viewport_size);
                }
            }
            MiddleButtonMode::DragPan => {
                if self.active_drag_pan.is_some() {
                    self.update_drag_pan(input_state, viewport_size);
                }
            }
        }
    }

    fn update_pivot_orbit(&mut self, input_state: &InputState, viewport_size: glam::Vec2) {
        let mouse_delta = input_state.get_mouse_delta();
        self.rotate_camera(mouse_delta);

        let Some(orbit) = self.active_pivot_orbit.as_ref() else {
            return;
        };
        let Some(direction_ws) = Self::screen_ray_direction(&self.camera, orbit.anchor_screen_pos, viewport_size)
        else {
            self.active_pivot_orbit = None;
            return;
        };

        // pivot 是交互锚点。反解相机位置而不是平移 pivot，才能保证锚点屏幕位置不漂移。
        self.camera.position = orbit.pivot_ws - direction_ws * orbit.distance;
    }

    fn update_drag_pan(&mut self, input_state: &InputState, viewport_size: glam::Vec2) {
        let Some(drag_pan) = self.active_drag_pan.as_ref() else {
            return;
        };
        let anchor_ws = drag_pan.anchor_ws;
        let distance = drag_pan.distance;
        let mouse_position = input_state.mouse_position();
        let screen_pos = glam::vec2(mouse_position[0] as f32, mouse_position[1] as f32);
        let Some(direction_ws) = Self::screen_ray_direction(&self.camera, screen_pos, viewport_size) else {
            self.active_drag_pan = None;
            return;
        };

        // 锚点代表拖拽开始时真正命中的场景位置；反解相机位置，保持该点始终位于当前鼠标射线上。
        self.camera.position = anchor_ws - direction_ws * distance;
    }

    fn update_wheel_zoom(&mut self, input_state: &InputState, viewport_size: glam::Vec2, delta_time_s: f32) {
        let wheel_delta = input_state.mouse_wheel_delta() as f32;
        if !wheel_delta.is_finite() {
            self.clear_wheel_zoom();
            return;
        }

        if wheel_delta.abs() <= f32::EPSILON {
            self.update_wheel_zoom_idle(delta_time_s);
            return;
        }

        if let Some(request) = self.pending_wheel_zoom_raycast.as_mut() {
            request.pending_wheel_delta += wheel_delta;
            return;
        }

        if self.active_wheel_zoom.is_some() && self.apply_wheel_zoom_delta(wheel_delta, viewport_size) {
            return;
        }

        self.pending_wheel_zoom_raycast =
            self.make_wheel_zoom_raycast(input_state.mouse_position(), viewport_size, wheel_delta);
    }

    fn update_wheel_zoom_idle(&mut self, delta_time_s: f32) {
        let Some(zoom) = self.active_wheel_zoom.as_mut() else {
            return;
        };

        zoom.idle_time_s += delta_time_s.max(0.0);
        if zoom.idle_time_s >= Self::WHEEL_ZOOM_IDLE_RESET_S {
            self.active_wheel_zoom = None;
        }
    }

    fn apply_wheel_zoom_delta(&mut self, wheel_delta: f32, viewport_size: glam::Vec2) -> bool {
        if wheel_delta.abs() <= f32::EPSILON {
            return true;
        }

        let Some(zoom) = self.active_wheel_zoom.as_ref() else {
            return false;
        };
        let anchor_ws = zoom.anchor_ws;
        let anchor_screen_pos = zoom.anchor_screen_pos;
        let current_distance = (anchor_ws - self.camera.position).length();
        if !current_distance.is_finite() || current_distance <= self.min_camera_distance() {
            self.active_wheel_zoom = None;
            return false;
        }

        let Some(direction_ws) = Self::screen_ray_direction(&self.camera, anchor_screen_pos, viewport_size) else {
            self.active_wheel_zoom = None;
            return false;
        };

        let zoom_exp = (-wheel_delta * Self::WHEEL_ZOOM_DISTANCE_SCALE_PER_DELTA)
            .clamp(-Self::WHEEL_ZOOM_EXP_LIMIT, Self::WHEEL_ZOOM_EXP_LIMIT);
        let new_distance =
            (current_distance * zoom_exp.exp()).clamp(self.min_camera_distance(), Self::SCREEN_RAY_T_MAX);
        self.camera.position = anchor_ws - direction_ws * new_distance;

        if let Some(zoom) = self.active_wheel_zoom.as_mut() {
            zoom.distance = new_distance;
            zoom.idle_time_s = 0.0;
        }
        true
    }

    fn clear_middle_button_control(&mut self) {
        self.clear_pivot_orbit();
        self.clear_drag_pan();
        self.active_middle_button_mode = None;
    }

    fn clear_pivot_orbit(&mut self) {
        self.pending_pivot_raycast = None;
        self.active_pivot_orbit = None;
    }

    fn clear_drag_pan(&mut self) {
        self.pending_drag_pan_raycast = None;
        self.active_drag_pan = None;
    }

    fn clear_wheel_zoom(&mut self) {
        self.pending_wheel_zoom_raycast = None;
        self.active_wheel_zoom = None;
    }

    fn min_camera_distance(&self) -> f32 {
        self.camera.near.max(0.001) * 2.0
    }

    fn rotate_camera(&mut self, mouse_delta: [f64; 2]) {
        self.camera.rotate_yaw(-mouse_delta[0] as f32 / Self::ROTATE_SENSITIVITY_DIVISOR);
        self.camera.rotate_pitch(-mouse_delta[1] as f32 / Self::ROTATE_SENSITIVITY_DIVISOR);
    }

    fn screen_ray_direction(camera: &Camera, screen_pos: glam::Vec2, viewport_size: glam::Vec2) -> Option<glam::Vec3> {
        if !screen_pos.is_finite()
            || !viewport_size.is_finite()
            || viewport_size.x <= 0.0
            || viewport_size.y <= 0.0
            || screen_pos.x < 0.0
            || screen_pos.y < 0.0
            || screen_pos.x >= viewport_size.x
            || screen_pos.y >= viewport_size.y
        {
            return None;
        }

        let uv = screen_pos / viewport_size;
        let ndc = glam::vec2(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
        let target_vs = camera.get_projection_matrix().inverse() * glam::vec4(ndc.x, ndc.y, 1.0, 1.0);
        let direction_vs = target_vs.truncate();
        if !direction_vs.is_finite() || direction_vs.length_squared() <= f32::EPSILON {
            return None;
        }

        let direction_ws = camera.get_view_matrix().inverse().transform_vector3(direction_vs.normalize());
        if !direction_ws.is_finite() || direction_ws.length_squared() <= f32::EPSILON {
            return None;
        }

        Some(direction_ws.normalize())
    }
}
