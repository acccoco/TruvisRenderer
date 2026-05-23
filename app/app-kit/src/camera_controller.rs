use truvis_app_frame::input_event::KeyCode;
use truvis_render_runtime::platform::camera::Camera;
use truvis_render_runtime::ray_cast::{RayCastRay, RayCastResult};

use crate::input_state::InputState;

pub struct CameraController {
    camera: Camera,
    pending_pivot_raycast: Option<PivotRayCastRequest>,
    active_pivot_orbit: Option<PivotOrbitState>,
}

/// 中键按下后交给 App 在 after_prepare 阶段执行的 pivot 查询。
#[derive(Clone, Copy, Debug)]
pub struct PivotRayCastRequest {
    pub ray: RayCastRay,
    anchor_screen_pos: glam::Vec2,
}

struct PivotOrbitState {
    pivot_ws: glam::Vec3,
    anchor_screen_pos: glam::Vec2,
    distance: f32,
}

impl Default for CameraController {
    fn default() -> Self {
        Self {
            camera: Camera::default(),
            pending_pivot_raycast: None,
            active_pivot_orbit: None,
        }
    }
}

impl CameraController {
    const ROTATE_SENSITIVITY_DIVISOR: f32 = 7.0;
    const MOVE_SPEED: f32 = 320.0;
    const SCREEN_RAY_T_MAX: f32 = 10000.0;

    pub fn camera(&self) -> &Camera {
        &self.camera
    }

    pub fn camera_mut(&mut self) -> &mut Camera {
        &mut self.camera
    }

    pub fn take_pending_pivot_raycast(&mut self) -> Option<PivotRayCastRequest> {
        self.pending_pivot_raycast.take()
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
        let delta_time_s = delta_time.as_secs_f32();

        self.camera.set_aspect_ratio(viewport_size.x / viewport_size.y);

        if !input_state.is_middle_button_pressed() {
            self.pending_pivot_raycast = None;
            self.active_pivot_orbit = None;
        } else {
            if input_state.is_middle_button_just_pressed() {
                self.active_pivot_orbit = None;
                self.pending_pivot_raycast = self.make_pivot_raycast(input_state.mouse_position(), viewport_size);
            }

            if self.active_pivot_orbit.is_some() {
                self.update_pivot_orbit(input_state, viewport_size);
            }
            return;
        }

        if input_state.is_right_button_pressed() {
            let mouse_delta = input_state.get_mouse_delta();
            self.rotate_camera(mouse_delta);
        }

        if input_state.is_key_pressed(KeyCode::KeyW) {
            self.camera.move_forward(delta_time_s * Self::MOVE_SPEED);
        }
        if input_state.is_key_pressed(KeyCode::KeyS) {
            self.camera.move_forward(-delta_time_s * Self::MOVE_SPEED);
        }
        if input_state.is_key_pressed(KeyCode::KeyA) {
            self.camera.move_right(-delta_time_s * Self::MOVE_SPEED);
        }
        if input_state.is_key_pressed(KeyCode::KeyD) {
            self.camera.move_right(delta_time_s * Self::MOVE_SPEED);
        }
        if input_state.is_key_pressed(KeyCode::KeyE) {
            self.camera.move_up(delta_time_s * Self::MOVE_SPEED);
        }
        if input_state.is_key_pressed(KeyCode::KeyQ) {
            self.camera.move_up(-delta_time_s * Self::MOVE_SPEED);
        }
    }

    fn make_pivot_raycast(&self, mouse_position: [f64; 2], viewport_size: glam::Vec2) -> Option<PivotRayCastRequest> {
        let anchor_screen_pos = glam::vec2(mouse_position[0] as f32, mouse_position[1] as f32);
        let ray = self.make_screen_raycast(mouse_position, viewport_size)?;
        Some(PivotRayCastRequest { ray, anchor_screen_pos })
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
