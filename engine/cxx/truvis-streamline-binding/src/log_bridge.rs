//! Streamline 日志回调到 Rust `log` facade 的桥接层。
//!
//! Streamline 的 `logMessageCallback` 可能出现在 `slInit`、`slShutdown` 或 Vulkan
//! interposer 调用栈中，因此这里把 FFI callback 设计成很薄的一层：只复制消息并尝试入队。
//! 真正的日志输出放到专用 drain 线程中完成，避免日志 IO 或 formatter 锁阻塞渲染线程。

use std::{
    ffi::{c_char, c_void},
    io,
    panic::{AssertUnwindSafe, catch_unwind},
    slice,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
        mpsc::{Receiver, SyncSender, TrySendError, sync_channel},
    },
    thread::{self, JoinHandle},
};

use crate::truvixx;

const STREAMLINE_LOG_TARGET: &str = "streamline";
const STREAMLINE_LOG_QUEUE_CAPACITY: usize = 1024;

#[derive(Clone, Copy, Debug)]
enum StreamlineLogType {
    Info,
    Warn,
    Error,
}

#[derive(Debug)]
struct StreamlineLogMessage {
    ty: StreamlineLogType,
    native_thread_id: u32,
    message: String,
}

struct StreamlineLogState {
    sender: SyncSender<StreamlineLogMessage>,
    dropped_count: Arc<AtomicUsize>,
}

/// Streamline 日志桥的 Rust 侧生命周期守卫。
///
/// C++ wrapper 会保存 `user_data` 裸指针并在 SL callback 中回传。该指针指向本类型持有的
/// `StreamlineLogState`，所以 `StreamlineRuntime` 必须在 `slShutdown` 返回后才能释放它。
pub(crate) struct StreamlineLogBridge {
    state: Option<Box<StreamlineLogState>>,
    drain_thread: Option<JoinHandle<()>>,
}

impl StreamlineLogBridge {
    pub(crate) fn new() -> io::Result<Self> {
        let (sender, receiver) = sync_channel(STREAMLINE_LOG_QUEUE_CAPACITY);
        let dropped_count = Arc::new(AtomicUsize::new(0));
        let drain_dropped_count = Arc::clone(&dropped_count);
        let drain_thread = thread::Builder::new()
            .name("streamline-log-drain".to_string())
            .spawn(move || drain_log_messages(receiver, drain_dropped_count))?;

        Ok(Self {
            state: Some(Box::new(StreamlineLogState { sender, dropped_count })),
            drain_thread: Some(drain_thread),
        })
    }

    pub(crate) fn callback() -> truvixx::TruvixxSlLogCallback {
        Some(streamline_log_callback)
    }

    pub(crate) fn user_data(&self) -> *mut c_void {
        self.state
            .as_ref()
            .map(|state| (&**state as *const StreamlineLogState).cast_mut().cast::<c_void>())
            .unwrap_or(std::ptr::null_mut())
    }
}

impl Drop for StreamlineLogBridge {
    fn drop(&mut self) {
        // 先释放 sender，让 drain 线程从 recv 中退出并 flush 已入队日志；随后 join，
        // 保证 StreamlineRuntime drop 返回前不会留下后台线程。
        self.state.take();

        if let Some(drain_thread) = self.drain_thread.take() {
            if let Err(payload) = drain_thread.join() {
                log::warn!(
                    target: STREAMLINE_LOG_TARGET,
                    "Streamline log drain thread panicked: {:?}",
                    payload
                );
            }
        }
    }
}

unsafe extern "C" fn streamline_log_callback(
    ty: truvixx::TruvixxSlLogType,
    message_utf8: *const c_char,
    message_len: u32,
    native_thread_id: u32,
    user_data: *mut c_void,
) {
    // Rust panic 不能跨过 C ABI 边界。这里吞掉 panic，避免 SL callback 把宿主进程带到
    // 未定义行为；真正的问题会在调试期通过 Rust panic hook 或日志 drain 侧暴露。
    let _ = catch_unwind(AssertUnwindSafe(|| unsafe {
        enqueue_log_message(ty, message_utf8, message_len, native_thread_id, user_data);
    }));
}

unsafe fn enqueue_log_message(
    ty: truvixx::TruvixxSlLogType,
    message_utf8: *const c_char,
    message_len: u32,
    native_thread_id: u32,
    user_data: *mut c_void,
) {
    if message_utf8.is_null() || user_data.is_null() {
        return;
    }

    let state = unsafe { &*(user_data.cast::<StreamlineLogState>()) };
    let message = unsafe { copy_streamline_message(message_utf8, message_len) };
    if message.is_empty() {
        return;
    }

    let log_message = StreamlineLogMessage {
        ty: map_log_type(ty),
        native_thread_id,
        message,
    };

    if let Err(TrySendError::Full(_)) = state.sender.try_send(log_message) {
        state.dropped_count.fetch_add(1, Ordering::Relaxed);
    }
}

unsafe fn copy_streamline_message(message_utf8: *const c_char, message_len: u32) -> String {
    let bytes = unsafe { slice::from_raw_parts(message_utf8.cast::<u8>(), message_len as usize) };
    String::from_utf8_lossy(bytes).trim_end_matches(|ch| ch == '\r' || ch == '\n').to_owned()
}

fn map_log_type(ty: truvixx::TruvixxSlLogType) -> StreamlineLogType {
    match ty {
        value if value == truvixx::TruvixxSlLogType_TruvixxSlLogTypeWarn => StreamlineLogType::Warn,
        value if value == truvixx::TruvixxSlLogType_TruvixxSlLogTypeError => StreamlineLogType::Error,
        _ => StreamlineLogType::Info,
    }
}

fn drain_log_messages(receiver: Receiver<StreamlineLogMessage>, dropped_count: Arc<AtomicUsize>) {
    while let Ok(message) = receiver.recv() {
        flush_dropped_count(&dropped_count);
        log_streamline_message(message);
    }

    flush_dropped_count(&dropped_count);
}

fn flush_dropped_count(dropped_count: &AtomicUsize) {
    let count = dropped_count.swap(0, Ordering::Relaxed);
    if count > 0 {
        log::warn!(
            target: STREAMLINE_LOG_TARGET,
            "Streamline log queue dropped {} messages because the callback queue was full.",
            count
        );
    }
}

fn log_streamline_message(message: StreamlineLogMessage) {
    match message.ty {
        StreamlineLogType::Info => {
            log::debug!(
                target: STREAMLINE_LOG_TARGET,
                "[sl_tid={}] {}",
                message.native_thread_id,
                message.message
            );
        }
        StreamlineLogType::Warn => {
            log::warn!(
                target: STREAMLINE_LOG_TARGET,
                "[sl_tid={}] {}",
                message.native_thread_id,
                message.message
            );
        }
        StreamlineLogType::Error => {
            log::error!(
                target: STREAMLINE_LOG_TARGET,
                "[sl_tid={}] {}",
                message.native_thread_id,
                message.message
            );
        }
    }
}
