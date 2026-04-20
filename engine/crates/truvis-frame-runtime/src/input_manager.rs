use truvis_app_api::input_event::{ElementState, InputEvent, MouseButton};
use crate::input_state::InputState;
use std::collections::VecDeque;

pub struct InputManager {
    state: InputState,
    events: VecDeque<InputEvent>,
}

impl Default for InputManager {
    fn default() -> Self {
        Self::new()
    }
}

impl InputManager {
    #[inline]
    pub fn state(&self) -> &InputState {
        &self.state
    }
}

impl InputManager {
    pub fn new() -> Self {
        Self {
            state: InputState::default(),
            events: VecDeque::new(),
        }
    }

    pub fn push_event(&mut self, event: InputEvent) {
        self.events.push_back(event);
    }

    pub fn get_events(&self) -> &VecDeque<InputEvent> {
        &self.events
    }

    pub fn process_events(&mut self) {
        self.state.last_mouse_pos = self.state.crt_mouse_pos;

        while let Some(event) = self.events.pop_front() {
            match event {
                InputEvent::KeyboardInput { key_code, state } => {
                    self.state.key_pressed.insert(key_code, state == ElementState::Pressed);
                }
                InputEvent::MouseButtonInput { button, state } => {
                    if button == MouseButton::Right {
                        self.state.right_button_pressed = state == ElementState::Pressed;
                    }
                }
                InputEvent::MouseMoved {
                    physical_position: position,
                } => {
                    self.state.crt_mouse_pos = position;
                }
                InputEvent::MouseWheel { delta: _ } => {}
                InputEvent::Resized { .. } => {}
                InputEvent::Other => {}
            }
        }
    }
}
