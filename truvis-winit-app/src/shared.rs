//! 主线程与渲染线程之间的共享状态与跨线程消息定义。

use std::any::Any;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64};

use crossbeam_channel::{Receiver, Sender, unbounded};
use raw_window_handle::{RawDisplayHandle, RawWindowHandle};

use truvis_app_api::input_event::InputEvent;

/// 跨线程传递 `!Send` 类型（如 `RawWindowHandle`）的受控包装。
///
/// # Safety
/// 调用者必须保证：
/// - 被包装的 handle 在接收方（渲染线程）使用期间，其所指向的底层资源（winit `Window`）
///   保持有效；
/// - 接收方仅将 handle 用于预期用途（例如传给 `ash_window::create_surface`），
///   不得解引用、转成线程特定资源或重复传递到其它线程。
///
/// 本项目中 `Window` 始终 own 在主线程，且二阶段关闭握手保证 `Window` 晚于
/// `VkSurfaceKHR` 销毁，因此跨线程传递 `RawWindowHandle` 的前提成立。
pub struct SendWrapper<T>(pub T);

// Safety: 由使用端通过生命周期顺序保证（见本结构体文档）。
unsafe impl<T> Send for SendWrapper<T> {}

/// 主线程通过 `AtomicU64` 发布的尺寸打包格式：高 32 位 width，低 32 位 height。
#[inline]
pub const fn pack_size(w: u32, h: u32) -> u64 {
    ((w as u64) << 32) | (h as u64)
}

#[inline]
pub const fn unpack_size(packed: u64) -> [u32; 2] {
    [(packed >> 32) as u32, packed as u32]
}

/// 主线程与渲染线程之间的共享状态。
///
/// 所有字段均可无锁访问（事件通道通过 crossbeam-channel 保证线程安全）。
pub struct SharedState {
    /// 主线程请求渲染线程退出；渲染线程每轮循环开头 `Acquire` 读取。
    pub exit: AtomicBool,

    /// 渲染线程销毁全部 Vulkan 资源后置位；主线程在 `about_to_wait` 中检测
    /// 并据此调用 `event_loop.exit()`。
    pub render_finished: AtomicBool,

    /// 最新窗口物理尺寸的打包值（见 [`pack_size`]）。主线程写、渲染线程读。
    pub size: AtomicU64,

    /// 渲染线程 panic 时存放 `catch_unwind` 捕获的 payload，供主线程 `resume_unwind`。
    pub panic_payload: Mutex<Option<Box<dyn Any + Send>>>,

    /// 主线程 → 渲染线程事件通道（非阻塞 unbounded）。
    pub event_sender: Sender<InputEvent>,
    pub event_receiver: Receiver<InputEvent>,
}

impl SharedState {
    pub fn new(initial_size: [u32; 2]) -> Self {
        let (event_sender, event_receiver) = unbounded();
        Self {
            exit: AtomicBool::new(false),
            render_finished: AtomicBool::new(false),
            size: AtomicU64::new(pack_size(initial_size[0], initial_size[1])),
            panic_payload: Mutex::new(None),
            event_sender,
            event_receiver,
        }
    }
}

/// 主线程在 window 创建完成后，通过单次 channel 投递给渲染线程的初始化消息。
///
/// raw handles 通过 [`SendWrapper`] 标记为 `Send`；渲染线程接收后用于
/// `ash_window::create_surface`，不得用于其它用途。
pub struct RenderInitMsg {
    pub raw_display: SendWrapper<RawDisplayHandle>,
    pub raw_window: SendWrapper<RawWindowHandle>,
    pub scale_factor: f64,
    pub initial_size: [u32; 2],
}
