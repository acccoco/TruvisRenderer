//! Streamline 日志回调到 Rust `log` facade 的桥接层。
//!
//! 架构：C++ 侧注册给 SL 的 callback 直接调用本模块的全局 `extern "C"` 函数。
//! 该函数只做消息复制和 try_send 入队；真正的日志输出由专用 drain 线程完成，
//! 避免日志 IO 阻塞 SL/Vulkan 调用栈。
//!
//! 线程安全由 `OnceLock<SyncSender>` 保证：sender 初始化一次后只读，
//! `SyncSender::try_send` 本身是线程安全的。

use std::{
    ffi::c_char,
    io,
    panic::{AssertUnwindSafe, catch_unwind},
    slice,
    sync::{
        OnceLock,
        atomic::{AtomicUsize, Ordering},
        mpsc::{SyncSender, TrySendError, sync_channel},
    },
    thread::{self, JoinHandle},
};

use crate::truvixx;

const STREAMLINE_LOG_TARGET: &str = "streamline";
const STREAMLINE_LOG_QUEUE_CAPACITY: usize = 1024;

/// 全局 sender。由 `StreamlineLogBridge::new()` 写入一次，进程生命周期内不变。
static LOG_SENDER: OnceLock<LogSenderState> = OnceLock::new();

struct LogSenderState {
    sender: SyncSender<LogEntry>,
    dropped_count: AtomicUsize,
}

/// Channel 消息类型。`Shutdown` 作为 poison pill 让 drain 线程退出。
enum LogEntry {
    Message(StreamlineLogMessage),
    Shutdown,
}

#[derive(Clone, Copy, Debug)]
enum StreamlineLogType {
    Info,
    Warn,
    Error,
}

#[derive(Debug)]
struct StreamlineLogMessage {
    ty: StreamlineLogType,
    message: String,
}

/// Streamline 日志桥的 Rust 侧生命周期守卫。
///
/// 持有 drain 线程 handle。Drop 时发送 Shutdown 信号并 join，
/// 保证所有已入队日志被 flush。
///
/// `StreamlineRuntime` 在 `slShutdown` 返回后才 drop 本类型，
/// 保证 shutdown 期间的最后几条日志也能被 drain。
pub(crate) struct StreamlineLogBridge {
    drain_thread: Option<JoinHandle<()>>,
}

impl StreamlineLogBridge {
    pub(crate) fn new() -> io::Result<Self> {
        let (sender, receiver) = sync_channel(STREAMLINE_LOG_QUEUE_CAPACITY);
        let state = LogSenderState {
            sender,
            dropped_count: AtomicUsize::new(0),
        };

        LOG_SENDER
            .set(state)
            .map_err(|_| io::Error::new(io::ErrorKind::AlreadyExists, "Streamline log bridge already initialized"))?;

        let drain_thread = thread::Builder::new().name("streamline-logger".to_string()).spawn(move || {
            while let Ok(entry) = receiver.recv() {
                match entry {
                    LogEntry::Message(message) => {
                        flush_dropped_count();
                        log_streamline_message(message);
                    }
                    LogEntry::Shutdown => {
                        flush_dropped_count();
                        break;
                    }
                }
            }
        })?;

        Ok(Self {
            drain_thread: Some(drain_thread),
        })
    }

    /// 返回全局 callback 函数地址，供传递给 C++ 的 `TruvixxSlInitDesc::log_callback`。
    pub(crate) fn callback() -> truvixx::TruvixxSlLogCallback {
        Some(truvixx_rust_log_callback)
    }
}

impl Drop for StreamlineLogBridge {
    fn drop(&mut self) {
        if let Some(state) = LOG_SENDER.get() {
            // 使用 send（阻塞）而不是 try_send，确保 Shutdown 一定送达。
            // 此时不再有渲染线程产生新日志（slShutdown 已返回），队列应有空位。
            let _ = state.sender.send(LogEntry::Shutdown);
        }

        if let Some(handle) = self.drain_thread.take() {
            if let Err(payload) = handle.join() {
                log::warn!(
                    target: STREAMLINE_LOG_TARGET,
                    "Streamline log drain thread panicked: {:?}",
                    payload
                );
            }
        }
    }
}

/// C++ 侧通过函数指针直接调用的全局日志入口。
///
/// 该函数在 SL/Vulkan 调用栈上执行，可能来自任意线程。
/// 只做消息复制和 try_send，不做 IO，不调用 SL API。
///
/// - `#[unsafe(no_mangle)]`：禁止 Rust 编译器对符号名做 name mangling，保证编译产物中
///   的符号名就是 `truvixx_rust_log_callback`。这样 C++ 侧通过函数指针调用时 ABI 兼容。
///   标记为 `unsafe` 是 Rust 2024 edition 的要求——暴露未修饰符号可能导致全局符号冲突，
///   编译器无法再帮助检测重名。
/// - `pub`：防止编译器把该函数视为 dead code 优化删除。当前虽然通过函数指针传递给 C++
///   （不走链接期符号解析），但 `pub` + `no_mangle` 组合确保函数始终存在于最终二进制中。
/// - `extern "C"`：使用 C 调用约定（参数传递方式、栈清理规则），使 C++ 调用兼容。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn truvixx_rust_log_callback(
    ty: truvixx::TruvixxSlLogType,
    message_utf8: *const c_char,
    message_len: u32,
) {
    let _ = catch_unwind(AssertUnwindSafe(|| unsafe {
        enqueue_log_message(ty, message_utf8, message_len);
    }));
}

unsafe fn enqueue_log_message(ty: truvixx::TruvixxSlLogType, message_utf8: *const c_char, message_len: u32) {
    if message_utf8.is_null() {
        return;
    }

    let Some(state) = LOG_SENDER.get() else {
        return;
    };

    let message = unsafe { copy_streamline_message(message_utf8, message_len) };
    if message.is_empty() {
        return;
    }

    let log_message = StreamlineLogMessage {
        ty: map_log_type(ty),
        message,
    };

    let entry = LogEntry::Message(log_message);
    if let Err(TrySendError::Full(_)) = state.sender.try_send(entry) {
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

fn flush_dropped_count() {
    let Some(state) = LOG_SENDER.get() else {
        return;
    };
    let count = state.dropped_count.swap(0, Ordering::Relaxed);
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
            log::debug!(target: STREAMLINE_LOG_TARGET, "{}", message.message);
        }
        StreamlineLogType::Warn => {
            log::warn!(target: STREAMLINE_LOG_TARGET, "{}", message.message);
        }
        StreamlineLogType::Error => {
            log::error!(target: STREAMLINE_LOG_TARGET, "{}", message.message);
        }
    }
}
