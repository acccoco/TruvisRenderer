//! Platform input event types.

#[derive(Debug, Hash, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Back,
    Forward,
    Other(u16),
}

#[derive(Debug, Hash, PartialEq, Eq, Clone, Copy)]
pub enum ElementState {
    Pressed,
    Released,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum KeyCode {
    KeyW,
    KeyA,
    KeyS,
    KeyD,
    KeyE,
    KeyQ,
    Other,
}

#[derive(Debug, Clone, PartialEq)]
pub enum InputEvent {
    KeyboardInput { key_code: KeyCode, state: ElementState },
    MouseButtonInput { button: MouseButton, state: ElementState },
    MouseMoved { physical_position: [f64; 2] },
    MouseWheel { delta: f64 },
    Resized { physical_width: u32, physical_height: u32 },
    Other,
}
