use std::collections::HashMap;

use truvis_frame_api::input_event::{ElementState, InputEvent, KeyCode, MouseButton};

#[derive(Default, Clone)]
pub struct InputState {
    pub crt_mouse_pos: [f64; 2],
    pub last_mouse_pos: [f64; 2],
    pub right_button_pressed: bool,
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
    }

    pub fn process_event(&mut self, event: &InputEvent) {
        match event {
            InputEvent::KeyboardInput { key_code, state } => {
                self.state.key_pressed.insert(*key_code, *state == ElementState::Pressed);
            }
            InputEvent::MouseButtonInput { button, state } => {
                if *button == MouseButton::Right {
                    self.state.right_button_pressed = *state == ElementState::Pressed;
                }
            }
            InputEvent::MouseMoved { physical_position } => {
                self.state.crt_mouse_pos = *physical_position;
            }
            InputEvent::MouseWheel { .. } | InputEvent::Resized { .. } | InputEvent::Other => {}
        }
    }
}
