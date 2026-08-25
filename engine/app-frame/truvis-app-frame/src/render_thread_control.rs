//! 平台窗口与固定帧执行器之间的最小线程控制契约。

use std::any::Any;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crossbeam_channel::{Receiver, Sender, unbounded};
use raw_window_handle::{RawDisplayHandle, RawWindowHandle};

use crate::input_event::InputEvent;

/// 窗口 owner 与 [`crate::RenderAppRunner`] 共享的线程控制状态。
///
/// 平台层只能通过窄方法发布输入、窗口尺寸和退出状态；渲染循环读取同一份
/// control，但不会获得平台窗口或具体 App。尺寸写入与 generation 的 Release / Acquire
/// 配合保持现有 latest-size 和 debounce 语义。
pub struct RenderThreadControl {
    /// 窗口 owner 发布退出请求；Runner 每轮循环开始时读取。
    exit: AtomicBool,

    /// 渲染线程完成显式 shutdown 后发布，供窗口事件循环决定何时退出。
    render_finished: AtomicBool,

    /// 最新物理尺寸，按照高 32 位 width、低 32 位 height 压入单个原子值。
    size: AtomicU64,

    /// 每次窗口 resize 都递增，即使宽高最终回到之前的值也会重新开始 debounce。
    resize_generation: AtomicU64,

    /// catch_unwind 捕获的 panic payload，由窗口 owner 在 join 后重新抛出。
    panic_payload: Mutex<Option<Box<dyn Any + Send>>>,

    /// 平台输入保持现有 unbounded channel；窗口线程不会等待渲染线程腾出容量。
    event_sender: Sender<InputEvent>,
    event_receiver: Receiver<InputEvent>,
}

impl RenderThreadControl {
    pub fn new(initial_size: [u32; 2]) -> Self {
        let (event_sender, event_receiver) = unbounded();
        Self {
            exit: AtomicBool::new(false),
            render_finished: AtomicBool::new(false),
            size: AtomicU64::new(Self::pack_size(initial_size)),
            resize_generation: AtomicU64::new(0),
            panic_payload: Mutex::new(None),
            event_sender,
            event_receiver,
        }
    }

    /// 发布退出请求；窗口 owner 不等待渲染线程，join 由显式关闭握手处理。
    pub fn request_exit(&self) {
        self.exit.store(true, Ordering::Release);
    }

    pub(crate) fn exit_requested(&self) -> bool {
        self.exit.load(Ordering::Acquire)
    }

    /// 先写入最新尺寸，再用 Release generation 发布，保持窗口事件与 Runner 的可见性顺序。
    pub fn publish_resize(&self, size: [u32; 2]) {
        self.size.store(Self::pack_size(size), Ordering::Relaxed);
        self.resize_generation.fetch_add(1, Ordering::Release);
    }

    pub(crate) fn latest_size(&self) -> [u32; 2] {
        Self::unpack_size(self.size.load(Ordering::Relaxed))
    }

    pub(crate) fn resize_generation(&self) -> u64 {
        self.resize_generation.load(Ordering::Acquire)
    }

    /// 保持现有 best-effort 输入投递语义；渲染线程退出后的断开事件直接忽略。
    pub fn send_input(&self, event: InputEvent) {
        let _ = self.event_sender.send(event);
    }

    pub(crate) fn try_receive_input(&self) -> Option<InputEvent> {
        self.event_receiver.try_recv().ok()
    }

    /// 标记 OS RenderThread 已退出固定帧执行器，并允许窗口 owner 结束消息循环。
    pub fn mark_finished(&self) {
        self.render_finished.store(true, Ordering::Release);
    }

    pub fn is_finished(&self) -> bool {
        self.render_finished.load(Ordering::Acquire)
    }

    /// 缓存渲染线程 panic，让窗口 owner 在 join 并释放窗口后延续原有失败语义。
    pub fn record_panic(&self, payload: Box<dyn Any + Send>) {
        if let Ok(mut slot) = self.panic_payload.lock() {
            *slot = Some(payload);
        }
    }

    pub fn take_panic(&self) -> Option<Box<dyn Any + Send>> {
        self.panic_payload.lock().ok().and_then(|mut payload| payload.take())
    }

    #[inline]
    const fn pack_size([width, height]: [u32; 2]) -> u64 {
        ((width as u64) << 32) | (height as u64)
    }

    #[inline]
    const fn unpack_size(packed: u64) -> [u32; 2] {
        [(packed >> 32) as u32, packed as u32]
    }
}

/// 固定帧执行器使用的一次性窗口初始化参数。
///
/// 本类型本身不声明 Send；平台 owner 负责受控跨线程传递，并只在 OS RenderThread
/// 内交给 [`crate::RenderAppRunner`] 创建 Vulkan surface。
pub struct RenderThreadInit {
    pub raw_display: RawDisplayHandle,
    pub raw_window: RawWindowHandle,
    pub scale_factor: f64,
    pub initial_size: [u32; 2],
}
