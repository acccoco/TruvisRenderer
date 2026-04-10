use std::io::Write;

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
            let black_style = info_style.fg_color(Some(anstyle::Color::Rgb(anstyle::RgbColor(75, 75, 75))));

            let line = record.line().unwrap_or(!0);
            let file = record.file().unwrap_or("").split("\\").last().unwrap_or("");
            let time = chrono::Local::now().format("%H:%M:%S");
            let level = record.level();
            // let module = record.module_path().unwrap_or("");

            writeln!(
                buf,
                "{level_style}[{time}] {level}{level_style:#} {grey_style}[{file}:{line}]{grey_style:#} \
                 {black_style}{}{black_style:#}",
                record.args()
            )
        })
        .filter(None, if cfg!(debug_assertions) { log::LevelFilter::Debug } else { log::LevelFilter::Info })
        .init();
}
