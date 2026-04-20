use truvis_app_api::input_event::{ElementState, InputEvent, KeyCode, MouseButton};
use winit::event::{KeyEvent, WindowEvent};
use winit::keyboard::PhysicalKey;

pub struct WinitEventAdapter {}
impl WinitEventAdapter {
    pub fn from_winit_event(event: &WindowEvent) -> InputEvent {
        match event {
            WindowEvent::CursorMoved { position, .. } => InputEvent::MouseMoved {
                physical_position: [position.x, position.y],
            },
            WindowEvent::MouseWheel { delta, .. } => {
                // 简化处理，仅考虑垂直滚动
                let delta_value = match delta {
                    winit::event::MouseScrollDelta::LineDelta(_, y) => *y as f64,
                    winit::event::MouseScrollDelta::PixelDelta(pos) => pos.y / 100.0,
                };
                InputEvent::MouseWheel { delta: delta_value }
            }
            WindowEvent::MouseInput { state, button, .. } => InputEvent::MouseButtonInput {
                button: Self::buttom_from_winit(*button),
                state: Self::state_from_winit(*state),
            },
            WindowEvent::KeyboardInput { event, .. } => {
                if let KeyEvent {
                    physical_key: PhysicalKey::Code(key_code),
                    state,
                    ..
                } = event
                {
                    InputEvent::KeyboardInput {
                        key_code: Self::key_from_winit(*key_code),
                        state: Self::state_from_winit(*state),
                    }
                } else {
                    InputEvent::Other
                }
            }
            WindowEvent::Resized(physical_size) => InputEvent::Resized {
                physical_width: physical_size.width,
                physical_height: physical_size.height,
            },
            _ => InputEvent::Other,
        }
    }

    fn buttom_from_winit(button: winit::event::MouseButton) -> MouseButton {
        match button {
            winit::event::MouseButton::Left => MouseButton::Left,
            winit::event::MouseButton::Right => MouseButton::Right,
            winit::event::MouseButton::Middle => MouseButton::Middle,
            winit::event::MouseButton::Back => MouseButton::Back,
            winit::event::MouseButton::Forward => MouseButton::Forward,
            winit::event::MouseButton::Other(code) => MouseButton::Other(code),
        }
    }

    fn key_from_winit(key: winit::keyboard::KeyCode) -> KeyCode {
        match key {
            winit::keyboard::KeyCode::KeyW => KeyCode::KeyW,
            winit::keyboard::KeyCode::KeyA => KeyCode::KeyA,
            winit::keyboard::KeyCode::KeyS => KeyCode::KeyS,
            winit::keyboard::KeyCode::KeyD => KeyCode::KeyD,
            winit::keyboard::KeyCode::KeyE => KeyCode::KeyE,
            winit::keyboard::KeyCode::KeyQ => KeyCode::KeyQ,
            _ => KeyCode::Other,
        }
    }

    fn state_from_winit(state: winit::event::ElementState) -> ElementState {
        match state {
            winit::event::ElementState::Pressed => ElementState::Pressed,
            winit::event::ElementState::Released => ElementState::Released,
        }
    }
}
