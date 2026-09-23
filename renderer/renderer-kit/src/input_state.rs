use std::collections::HashMap;

use truvis_render_loop::input_event::{ElementState, InputEvent, KeyCode, MouseButton};

#[derive(Default, Clone)]
pub struct InputState {
    pub crt_mouse_pos: [f64; 2],
    pub last_mouse_pos: [f64; 2],
    pub left_button_pressed: bool,
    pub left_button_just_pressed: bool,
    pub left_button_just_released: bool,
    /// 同帧后续 MouseMoved 不得改变按下命中点，否则快速拖动可能选中背景网格。
    pub left_button_press_position: [f64; 2],
    pub right_button_pressed: bool,
    pub middle_button_pressed: bool,
    pub middle_button_just_pressed: bool,
    pub middle_button_just_released: bool,
    pub mouse_wheel_delta: f64,
    pub key_pressed: HashMap<KeyCode, bool>,
}

impl InputState {
    /// 相机手势占用时不启动对象选择。
    pub fn is_navigating(&self) -> bool {
        self.right_button_pressed
            || self.middle_button_pressed
            || self.mouse_wheel_delta != 0.0
            || [
                KeyCode::KeyW,
                KeyCode::KeyA,
                KeyCode::KeyS,
                KeyCode::KeyD,
                KeyCode::KeyQ,
                KeyCode::KeyE,
            ]
            .into_iter()
            .any(|key| self.is_key_pressed(key))
    }

    pub fn is_key_pressed(&self, key_code: KeyCode) -> bool {
        self.key_pressed.get(&key_code).copied().unwrap_or(false)
    }

    pub fn is_shift_pressed(&self) -> bool {
        self.is_key_pressed(KeyCode::ShiftLeft) || self.is_key_pressed(KeyCode::ShiftRight)
    }

    pub fn get_mouse_delta(&self) -> [f64; 2] {
        [
            self.crt_mouse_pos[0] - self.last_mouse_pos[0],
            self.crt_mouse_pos[1] - self.last_mouse_pos[1],
        ]
    }

    pub fn is_right_button_pressed(&self) -> bool {
        self.right_button_pressed
    }

    pub fn is_left_button_pressed(&self) -> bool {
        self.left_button_pressed
    }

    pub fn is_left_button_just_pressed(&self) -> bool {
        self.left_button_just_pressed
    }

    pub fn is_left_button_just_released(&self) -> bool {
        self.left_button_just_released
    }

    pub fn is_middle_button_pressed(&self) -> bool {
        self.middle_button_pressed
    }

    pub fn is_middle_button_just_pressed(&self) -> bool {
        self.middle_button_just_pressed
    }

    pub fn is_middle_button_just_released(&self) -> bool {
        self.middle_button_just_released
    }

    pub fn mouse_position(&self) -> [f64; 2] {
        self.crt_mouse_pos
    }

    pub fn mouse_wheel_delta(&self) -> f64 {
        self.mouse_wheel_delta
    }
}

#[derive(Default)]
pub struct InputManager {
    state: InputState,
}

impl InputManager {
    pub fn state(&self) -> &InputState {
        &self.state
    }

    pub fn reset(&mut self) {
        let position = self.state.crt_mouse_pos;
        self.state = InputState {
            crt_mouse_pos: position,
            last_mouse_pos: position,
            ..Default::default()
        };
    }

    pub fn begin_frame(&mut self) {
        self.state.last_mouse_pos = self.state.crt_mouse_pos;
        self.state.left_button_just_pressed = false;
        self.state.left_button_just_released = false;
        self.state.middle_button_just_pressed = false;
        self.state.middle_button_just_released = false;
        self.state.mouse_wheel_delta = 0.0;
    }

    pub fn process_event(&mut self, event: &InputEvent) {
        match event {
            InputEvent::Focused(false) | InputEvent::Resized { .. } => self.reset(),
            InputEvent::KeyboardInput { key_code, state } => {
                self.state.key_pressed.insert(*key_code, *state == ElementState::Pressed);
            }
            InputEvent::MouseButtonInput { button, state } => {
                let pressed = *state == ElementState::Pressed;
                match button {
                    MouseButton::Left => {
                        if pressed {
                            self.state.left_button_just_pressed = !self.state.left_button_pressed;
                            if self.state.left_button_just_pressed {
                                self.state.left_button_press_position = self.state.crt_mouse_pos;
                            }
                            self.state.left_button_pressed = true;
                        } else {
                            self.state.left_button_just_released = self.state.left_button_pressed;
                            self.state.left_button_pressed = false;
                        }
                    }
                    MouseButton::Right => {
                        self.state.right_button_pressed = pressed;
                    }
                    MouseButton::Middle => {
                        if pressed {
                            self.state.middle_button_just_pressed = !self.state.middle_button_pressed;
                            self.state.middle_button_pressed = true;
                        } else {
                            self.state.middle_button_just_released = self.state.middle_button_pressed;
                            self.state.middle_button_pressed = false;
                        }
                    }
                    _ => {}
                }
            }
            InputEvent::MouseMoved { physical_position } => {
                self.state.crt_mouse_pos = *physical_position;
            }
            InputEvent::MouseWheel { delta } => {
                self.state.mouse_wheel_delta += *delta;
            }
            InputEvent::Focused(true) | InputEvent::Other => {}
        }
    }
}
