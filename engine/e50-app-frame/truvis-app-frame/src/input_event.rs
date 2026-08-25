//! 平台输入事件的引擎侧表示。
//!
//! 平台层负责把 winit 等后端事件转换成这些窄类型，渲染线程和 App hooks 只处理
//! 这里定义的稳定输入模型。该模型当前只覆盖引擎 demo 已使用的键鼠输入；未知或
//! 暂未建模的平台事件通过 `Other` 保留兼容入口。

/// 鼠标按键的跨平台抽象。
///
/// 常见按键使用具名枚举，平台特有或额外按键保留原始编号放入 [`MouseButton::Other`]。
#[derive(Debug, Hash, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
pub enum MouseButton {
    /// 鼠标左键。
    Left,
    /// 鼠标右键。
    Right,
    /// 鼠标中键或滚轮按键。
    Middle,
    /// 浏览器/设备上的后退侧键。
    Back,
    /// 浏览器/设备上的前进侧键。
    Forward,
    /// 未显式建模的其他鼠标按键编号。
    Other(u16),
}

/// 按键或鼠标按钮的按下/释放状态。
#[derive(Debug, Hash, PartialEq, Eq, Clone, Copy)]
pub enum ElementState {
    /// 本帧收到按下事件。
    Pressed,
    /// 本帧收到释放事件。
    Released,
}

/// 引擎当前关心的物理键位。
///
/// 这里使用物理键位而不是文本字符，保证键盘布局变化时 WASD 等相机控制仍绑定
/// 到同一组物理按键。未列出的键位统一映射为 [`KeyCode::Other`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum KeyCode {
    /// 物理 W 键。
    KeyW,
    /// 物理 A 键。
    KeyA,
    /// 物理 S 键。
    KeyS,
    /// 物理 D 键。
    KeyD,
    /// 物理 E 键。
    KeyE,
    /// 物理 Q 键。
    KeyQ,
    /// 左 Shift 键。
    ShiftLeft,
    /// 右 Shift 键。
    ShiftRight,
    /// 未显式建模的其他键位。
    Other,
}

/// render thread 接收的输入事件。
///
/// 坐标和尺寸均使用平台窗口的物理像素坐标。滚轮 delta 已由平台适配层规整为
/// 垂直滚动量，当前不保留水平滚动分量。resize 事件用于 App/input 状态观察；
/// swapchain 重建仍由 render loop 的 latest-size 路径驱动。
#[derive(Debug, Clone, PartialEq)]
pub enum InputEvent {
    /// 键盘按键状态变化。
    KeyboardInput { key_code: KeyCode, state: ElementState },
    /// 鼠标按键状态变化。
    MouseButtonInput { button: MouseButton, state: ElementState },
    /// 鼠标指针移动到窗口内的物理像素坐标。
    MouseMoved { physical_position: [f64; 2] },
    /// 垂直滚轮输入，正负方向沿用平台适配层约定。
    MouseWheel { delta: f64 },
    /// 窗口物理尺寸变化事件。
    Resized { physical_width: u32, physical_height: u32 },
    /// 暂未建模或 App 当前不关心的平台事件。
    Other,
}
