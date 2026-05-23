use std::collections::HashMap;

use truvis_app_frame::input_event::{ElementState, InputEvent, KeyCode, MouseButton};

#[derive(Default, Clone)]
pub struct InputState {
    pub crt_mouse_pos: [f64; 2],
    pub last_mouse_pos: [f64; 2],
    pub right_button_pressed: bool,
    pub middle_button_pressed: bool,
    pub middle_button_just_pressed: bool,
    pub middle_button_just_released: bool,
    pub key_pressed: HashMap<KeyCode, bool>,
}

impl InputState {
    pub fn is_key_pressed(&self, key_code: KeyCode) -> bool {
        self.key_pressed.get(&key_code).copied().unwrap_or(false)
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
}

#[derive(Default)]
pub struct InputManager {
    state: InputState,
}

impl InputManager {
    pub fn state(&self) -> &InputState {
        &self.state
    }

    pub fn begin_frame(&mut self) {
        self.state.last_mouse_pos = self.state.crt_mouse_pos;
        self.state.middle_button_just_pressed = false;
        self.state.middle_button_just_released = false;
    }

    pub fn process_event(&mut self, event: &InputEvent) {
        match event {
            InputEvent::KeyboardInput { key_code, state } => {
                self.state.key_pressed.insert(*key_code, *state == ElementState::Pressed);
            }
            InputEvent::MouseButtonInput { button, state } => {
                let pressed = *state == ElementState::Pressed;
                match button {
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
            InputEvent::MouseWheel { .. } | InputEvent::Resized { .. } | InputEvent::Other => {}
        }
    }
}
