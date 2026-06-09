//! Truvis 日志初始化入口。
//!
//! 日志格式由本 crate 统一维护。每条日志会输出当前线程名称和 Windows 系统线程 ID（`GetCurrentThreadId`），
//! 线程上下文通过 thread-local 缓存，保证同一线程只在首次写日志时捕获名称和 tid。

use std::{
    env, fs,
    fs::{File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    process, thread,
};

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

struct TeeWriter {
    console: anstream::Stderr,
    file: anstream::StripStream<File>,
}

impl TeeWriter {
    fn new(file: File) -> Self {
        Self {
            console: anstream::stderr(),
            file: anstream::StripStream::new(file),
        }
    }
}

impl Write for TeeWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.console.write_all(buf)?;
        self.file.write_all(buf)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.console.flush()?;
        self.file.flush()
    }
}

/// 根据当前 exe 名称生成默认日志文件路径。
///
/// `temp_dir` 由调用方传入，通常来自 `TruvisPath::temp_dir()`。本 crate 不直接依赖
/// `truvis-path`，避免基础层同层 crate 之间出现不必要的路径依赖。
pub fn current_exe_log_file_path(temp_dir: impl AsRef<Path>) -> PathBuf {
    let exe_name = env::current_exe()
        .ok()
        .and_then(|path| path.file_stem().map(|stem| stem.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "truvis".to_owned());
    default_log_file_path(temp_dir, &exe_name)
}

/// 生成 `.temp/logs/{exe_name}-{time}-{pid}.log` 风格的日志文件路径。
pub fn default_log_file_path(temp_dir: impl AsRef<Path>, exe_name: &str) -> PathBuf {
    let exe_name = sanitize_file_stem(exe_name);
    let time = chrono::Local::now().format("%Y%m%d-%H%M%S");
    temp_dir.as_ref().join("logs").join(format!("{exe_name}-{time}-{}.log", process::id()))
}

fn sanitize_file_stem(stem: &str) -> String {
    let safe_stem = stem
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') { ch } else { '_' })
        .collect::<String>();

    if safe_stem.is_empty() { "truvis".to_owned() } else { safe_stem }
}

pub fn init_log() {
    init_log_with_target(None);
}

pub fn init_log_with_file(log_file_path: impl AsRef<Path>) {
    let log_file_path = log_file_path.as_ref();
    match open_log_file(log_file_path) {
        Ok(file) => init_log_with_target(Some(env_logger::Target::Pipe(Box::new(TeeWriter::new(file))))),
        Err(err) => {
            eprintln!("Failed to initialize file log {}: {}; fallback to console only.", log_file_path.display(), err);
            init_log();
        }
    }
}

fn open_log_file(log_file_path: &Path) -> io::Result<File> {
    if let Some(parent) = log_file_path.parent() {
        fs::create_dir_all(parent)?;
    }
    OpenOptions::new().create(true).append(true).open(log_file_path)
}

fn init_log_with_target(target: Option<env_logger::Target>) {
    let mut builder = env_logger::Builder::new();
    builder
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
        .filter(None, if cfg!(debug_assertions) { log::LevelFilter::Debug } else { log::LevelFilter::Info });

    if let Some(target) = target {
        // `Target::Pipe` 在 env_logger 中不能自动探测终端能力；这里必须生成 ANSI，
        // 再由 `TeeWriter` 分别交给 console 适配和 file strip。
        builder.target(target).write_style(env_logger::WriteStyle::Always);
    }

    builder.init();
}
