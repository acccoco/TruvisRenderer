use std::collections::HashMap;
use truvis_app_api::input_event::KeyCode;

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

    pub fn get_mouse_position(&self) -> [f64; 2] {
        self.crt_mouse_pos
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
