//! Truvis 日志初始化入口。
//!
//! 本 crate 位于 foundation 层，只维护项目统一日志系统的基础能力：`env_logger` 初始化、
//! console/file 双输出、日志格式和文件保留策略。调用方负责决定 `.temp` 根目录并把日志路径传进来，
//! 因此这里不直接依赖 `truvis-path`，避免基础层同层 crate 之间形成不必要的路径依赖。
//!
//! 日志格式由本 crate 统一维护。每条日志会输出当前线程名称和 Windows 系统线程 ID（`GetCurrentThreadId`），
//! 线程上下文通过 thread-local 缓存，保证同一线程只在首次写日志时捕获名称和 tid。

use std::{
    env, fs,
    fs::{File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    process, thread,
    time::{Duration, SystemTime},
};

const DEFAULT_RETAINED_LOG_FILES: usize = 3;

/// 项目统一日志初始化入口。
///
/// 这个类型承载对外 API：调用方只选择 console-only 还是 console + file，具体 formatter、双写 writer、
/// 文件保留策略和 `env_logger` 安装顺序都封装在本 crate 内部。这里刻意不保存全局状态，
/// 因为 `env_logger` 自身已经通过 `log` facade 管理全局 logger 安装。
pub struct TruvisLogger;

impl TruvisLogger {
    pub fn init() {
        EnvLoggerInstaller::install(None);
    }

    pub fn init_with_file(log_file_path: impl AsRef<Path>) {
        let log_file_path = log_file_path.as_ref();
        match LogFileOutput::open(log_file_path) {
            Ok(file) => {
                LogRetentionPolicy::new(DEFAULT_RETAINED_LOG_FILES).retain_for_current_log(log_file_path);
                EnvLoggerInstaller::install(Some(env_logger::Target::Pipe(Box::new(TeeWriter::new(file)))))
            }
            Err(err) => {
                eprintln!(
                    "Failed to initialize file log {}: {}; fallback to console only.",
                    log_file_path.display(),
                    err
                );
                Self::init();
            }
        }
    }
}

/// 默认文件日志路径生成器。
///
/// 路径生成保持为独立 public 类型，而不是塞进 `TruvisLogger`，因为它只表达命名协议：
/// `{exe_name}-{YYYYMMDD}-{HHMMSS}-{pid}.log`。调用方仍然负责传入 `.temp` 根目录，
/// 这样 `truvis-logs` 不需要依赖 `truvis-path`。
pub struct LogFilePath;

impl LogFilePath {
    pub fn current_exe(temp_dir: impl AsRef<Path>) -> PathBuf {
        let exe_name = env::current_exe()
            .ok()
            .and_then(|path| path.file_stem().map(|stem| stem.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "truvis".to_owned());
        Self::for_exe(temp_dir, &exe_name)
    }

    pub fn for_exe(temp_dir: impl AsRef<Path>, exe_name: &str) -> PathBuf {
        let exe_name = Self::sanitize_stem(exe_name);
        let time = chrono::Local::now().format("%Y%m%d-%H%M%S");
        temp_dir.as_ref().join("logs").join(format!("{exe_name}-{time}-{}.log", process::id()))
    }

    fn sanitize_stem(stem: &str) -> String {
        let safe_stem = stem
            .chars()
            .map(|ch| if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') { ch } else { '_' })
            .collect::<String>();

        if safe_stem.is_empty() { "truvis".to_owned() } else { safe_stem }
    }
}

/// 单个写日志线程的稳定上下文。
///
/// formatter 可能在主线程、渲染线程或 Streamline 日志转发线程上执行。线程名称和 Windows tid
/// 在第一次写日志时捕获并缓存到 thread-local，后续同一线程复用这份上下文，避免每条日志都重复查询系统线程信息。
/// 这个缓存也意味着：如果线程创建后再改名，日志里不会自动刷新名称；当前项目线程名称通常在创建时确定。
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

#[derive(Clone, Debug, PartialEq, Eq)]
struct LogFileName {
    group_name: String,
    timestamp_key: String,
}

impl LogFileName {
    /// 解析 `LogFilePath::for_exe()` 生成的 `{exe}-{YYYYMMDD}-{HHMMSS}-{pid}.log` 文件名协议。
    ///
    /// exe 名称本身允许包含 `-`，因此必须从右侧解析固定的 date/time/pid 三段，
    /// 保证 `cxx-build-20260611-105933-30368.log` 的分组名仍是 `cxx-build`。
    fn parse(path: &Path) -> Option<Self> {
        if !path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("log"))
        {
            return None;
        }

        let stem = path.file_stem()?.to_str()?;
        let mut parts = stem.rsplitn(4, '-');
        let pid = parts.next()?;
        let time = parts.next()?;
        let date = parts.next()?;
        let group_name = parts.next()?;

        if group_name.is_empty()
            || !Self::is_exact_digits(date, 8)
            || !Self::is_exact_digits(time, 6)
            || !Self::is_non_empty_digits(pid)
        {
            return None;
        }

        Some(Self {
            group_name: group_name.to_owned(),
            timestamp_key: format!("{date}-{time}"),
        })
    }

    fn is_exact_digits(value: &str, expected_len: usize) -> bool {
        value.len() == expected_len && Self::is_non_empty_digits(value)
    }

    fn is_non_empty_digits(value: &str) -> bool {
        !value.is_empty() && value.chars().all(|ch| ch.is_ascii_digit())
    }
}

/// 日志保留策略中的候选文件。
///
/// `LogRetentionPolicy` 只关心同一 exe 分组内的 `.log` 文件。这里把路径、分组名、当前文件标记、
/// 文件名时间戳和 modified fallback 收在同一个值里，避免排序和删除流程反复拆解文件名。
#[derive(Clone, Debug, PartialEq, Eq)]
struct RetainedLogFile {
    path: PathBuf,
    file_name: String,
    is_current: bool,
    group_name: String,
    timestamp_key: String,
    modified_key: Option<Duration>,
}

impl RetainedLogFile {
    fn from_dir_entry(entry: &fs::DirEntry, current_file_name: Option<&str>) -> Option<Self> {
        let path = entry.path();
        let log_file_name = LogFileName::parse(&path)?;
        let file_name = path.file_name().and_then(|file_name| file_name.to_str()).unwrap_or_default().to_owned();
        let modified_key =
            entry.metadata().ok().and_then(|metadata| metadata.modified().ok()).and_then(Self::system_time_key);

        Some(Self {
            is_current: Some(file_name.as_str()) == current_file_name,
            path,
            file_name,
            group_name: log_file_name.group_name,
            timestamp_key: log_file_name.timestamp_key,
            modified_key,
        })
    }

    fn belongs_to_group(&self, group_name: &str) -> bool {
        self.group_name == group_name
    }

    fn sort_newest_first(candidates: &mut [Self]) {
        // 当前文件已经被打开并准备接收本次进程日志，必须先计入保留集合，再按文件名时间戳保留旧文件。
        candidates.sort_by(|left, right| {
            right
                .is_current
                .cmp(&left.is_current)
                .then_with(|| right.timestamp_key.cmp(&left.timestamp_key))
                .then_with(|| right.modified_key.cmp(&left.modified_key))
                .then_with(|| right.file_name.cmp(&left.file_name))
        });
    }

    fn system_time_key(time: SystemTime) -> Option<Duration> {
        time.duration_since(SystemTime::UNIX_EPOCH).ok()
    }
}

/// 文件日志保留策略。
///
/// 这个策略是日志初始化路径的一部分：它在 file logger 打开成功后、`env_logger` 真正安装前执行。
/// 因此这里不能使用 `log` facade，也不能因为清理失败阻断应用启动；所有清理错误都通过 `eprintln!`
/// 作为 best-effort 诊断输出。当前进程打开的日志文件会优先进入保留集合，避免 public API 传入旧路径时误删正在写的文件。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LogRetentionPolicy {
    retained_files: usize,
}

impl LogRetentionPolicy {
    const fn new(retained_files: usize) -> Self {
        Self { retained_files }
    }

    fn retain_for_current_log(self, current_log_file_path: &Path) {
        if let Err(err) = self.try_retain_for_current_log(current_log_file_path) {
            eprintln!("Failed to retain recent log files for {}: {}.", current_log_file_path.display(), err);
        }
    }

    fn try_retain_for_current_log(self, current_log_file_path: &Path) -> io::Result<()> {
        let Some(current_log_name) = LogFileName::parse(current_log_file_path) else {
            return Ok(());
        };
        let Some(log_dir) = current_log_file_path.parent() else {
            return Ok(());
        };
        let current_file_name = current_log_file_path.file_name().and_then(|file_name| file_name.to_str());

        let mut candidates =
            self.collect_candidates(log_dir, current_file_name, current_log_name.group_name.as_str())?;
        RetainedLogFile::sort_newest_first(candidates.as_mut_slice());
        self.remove_stale_logs(candidates);
        Ok(())
    }

    fn collect_candidates(
        self,
        log_dir: &Path,
        current_file_name: Option<&str>,
        current_group_name: &str,
    ) -> io::Result<Vec<RetainedLogFile>> {
        let mut candidates = Vec::new();
        for entry in fs::read_dir(log_dir)? {
            let entry = match entry {
                Ok(entry) => entry,
                Err(err) => {
                    eprintln!("Failed to read log directory entry in {}: {}.", log_dir.display(), err);
                    continue;
                }
            };

            let Some(candidate) = RetainedLogFile::from_dir_entry(&entry, current_file_name) else {
                continue;
            };
            if candidate.belongs_to_group(current_group_name) {
                candidates.push(candidate);
            }
        }
        Ok(candidates)
    }

    fn remove_stale_logs(self, candidates: Vec<RetainedLogFile>) {
        for stale_log in candidates.into_iter().skip(self.retained_files) {
            if let Err(err) = fs::remove_file(&stale_log.path) {
                eprintln!("Failed to remove stale log file {}: {}.", stale_log.path.display(), err);
            }
        }
    }
}

/// console + file 双写 writer。
///
/// `env_logger::Target::Pipe` 只能接收一个 `Write` 目标，这里把同一条日志同时写给 stderr 和文件。
/// formatter 先生成带 ANSI style 的文本：console 端交给 `anstream` 适配终端能力，file 端通过 `StripStream`
/// 移除 escape sequence，保证落盘日志是纯文本。
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

/// 文件日志输出目标。
///
/// 打开文件和创建父目录是 file backend 的资源边界；失败时由 `TruvisLogger` 负责 fallback 到 console-only。
struct LogFileOutput;

impl LogFileOutput {
    fn open(log_file_path: &Path) -> io::Result<File> {
        if let Some(parent) = log_file_path.parent() {
            fs::create_dir_all(parent)?;
        }
        OpenOptions::new().create(true).append(true).open(log_file_path)
    }
}

/// `env_logger` 安装器。
///
/// 这个类型集中维护 filter、formatter、target 和 ANSI write style 的安装顺序。它不做路径选择，
/// 也不做文件 IO，因此 console-only 和 file backend 可以共享完全一致的 formatter。
struct EnvLoggerInstaller;

impl EnvLoggerInstaller {
    fn install(target: Option<env_logger::Target>) {
        let mut builder = env_logger::Builder::new();
        builder
            .format(LogFormatter::format)
            .filter(None, if cfg!(debug_assertions) { log::LevelFilter::Debug } else { log::LevelFilter::Info });

        if let Some(target) = target {
            // `Target::Pipe` 在 env_logger 中不能自动探测终端能力；这里必须生成 ANSI，
            // 再由 `TeeWriter` 分别交给 console 适配和 file strip。
            builder.target(target).write_style(env_logger::WriteStyle::Always);
        }

        builder.init();
    }
}

/// 单条日志格式化器。
///
/// formatter 只处理“如何展示一条 record”，线程上下文捕获交给 `ThreadLogContext`，
/// 输出目标交给 `EnvLoggerInstaller` / `TeeWriter`。这样格式规则可以保持集中，业务侧继续只使用 `log` facade。
struct LogFormatter;

impl LogFormatter {
    fn format(buf: &mut env_logger::fmt::Formatter, record: &log::Record<'_>) -> io::Result<()> {
        let info_style =
            buf.default_level_style(log::Level::Info).fg_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::Green)));
        let warn_style =
            buf.default_level_style(log::Level::Warn).fg_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::Yellow)));
        let error_style =
            buf.default_level_style(log::Level::Error).fg_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::Red)));

        let level_style = match record.level() {
            log::Level::Info => info_style,
            log::Level::Warn => warn_style,
            log::Level::Error => error_style,
            _ => buf.default_level_style(record.level()),
        };
        let grey_style = info_style.fg_color(Some(anstyle::Color::Rgb(anstyle::RgbColor(110, 110, 110))));

        let line = record.line().unwrap_or(!0);
        let file = record.file().unwrap_or("");
        let time = chrono::Local::now().format("%H:%M:%S");
        let level = record.level();
        let module = record.module_path().unwrap_or("");

        ThreadLogContext::with_thread_log_context(|thread_ctx| {
            writeln!(
                buf,
                "{level_style}[{time}] {level} [{thread_name}({tid})] {}{level_style:#}\n\t {grey_style}In {module} \
                 At {file}:{line}{grey_style:#}",
                record.args(),
                thread_name = thread_ctx.name.as_str(),
                tid = thread_ctx.tid.as_str()
            )
        })
    }
}
