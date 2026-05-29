//! Truvis 日志初始化入口。
//!
//! 日志格式由本 crate 统一维护。每条日志会输出当前线程名称和 Windows 系统线程 ID（`GetCurrentThreadId`），
//! 线程上下文通过 thread-local 缓存，保证同一线程只在首次写日志时捕获名称和 tid。

use std::{io::Write, thread};

#[derive(Clone, Debug, PartialEq, Eq)]
struct ThreadLogContext {
    name: String,
    tid: String,
}

impl ThreadLogContext {
    const UNNAMED_THREAD_NAME: &str = "unnamed";

    fn capture() -> Self {
        Self {
            name: Self::capture_thread_name(),
            tid: Self::capture_thread_id(),
        }
    }

    fn with_thread_log_context<R>(f: impl FnOnce(&Self) -> R) -> R {
        THREAD_LOG_CONTEXT.with(f)
    }

    fn capture_thread_name() -> String {
        let current = thread::current();
        current.name().unwrap_or(Self::UNNAMED_THREAD_NAME).to_owned()
    }

    fn capture_thread_id() -> String {
        unsafe extern "system" {
            fn GetCurrentThreadId() -> u32;
        }
        unsafe { GetCurrentThreadId() }.to_string()
    }
}

thread_local! {
    static THREAD_LOG_CONTEXT: ThreadLogContext = ThreadLogContext::capture();
}

pub fn init_log() {
    env_logger::Builder::new()
        .format(|buf, record| {
            let info_style = buf
                .default_level_style(log::Level::Info)
                .fg_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::Green)));
            let warn_style = buf
                .default_level_style(log::Level::Warn)
                .fg_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::Yellow)));
            let error_style = buf
                .default_level_style(log::Level::Error)
                .fg_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::Red)));

            let level_style = match record.level() {
                log::Level::Info => info_style,
                log::Level::Warn => warn_style,
                log::Level::Error => error_style,
                _ => buf.default_level_style(record.level()),
            };
            let grey_style = info_style.fg_color(Some(anstyle::Color::Rgb(anstyle::RgbColor(110, 110, 110))));
            let _black_style = info_style.fg_color(Some(anstyle::Color::Rgb(anstyle::RgbColor(75, 75, 75))));

            let line = record.line().unwrap_or(!0);
            let file = record.file().unwrap_or("");
            let _file_name = file.split("\\").last().unwrap_or("");
            let time = chrono::Local::now().format("%H:%M:%S");
            let level = record.level();
            let module = record.module_path().unwrap_or("");

            ThreadLogContext::with_thread_log_context(|thread_ctx| {
                writeln!(
                    buf,
                    "{level_style}[{time}] {level} [{thread_name}({tid})] {}{level_style:#}\n\t {grey_style}In \
                     {module} At {file}:{line}{grey_style:#}",
                    record.args(),
                    thread_name = thread_ctx.name.as_str(),
                    tid = thread_ctx.tid.as_str()
                )
            })
        })
        .filter(None, if cfg!(debug_assertions) { log::LevelFilter::Debug } else { log::LevelFilter::Info })
        .init();
}
