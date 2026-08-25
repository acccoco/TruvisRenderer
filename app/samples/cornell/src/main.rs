use cornell::cornell_render_app::CornellRenderApp;
use truvis_app_frame::init_env_with_log_file;
use truvis_logs::LogFilePath;
use truvis_path::TruvisPath;
use truvis_winit_host::{StandaloneWindowOptions, StandaloneWinitHost};

fn main() {
    init_env_with_log_file(LogFilePath::current_exe(TruvisPath::temp_dir()));

    let options = StandaloneWindowOptions {
        title: "Truvis".to_string(),
        logical_size: [1200.0, 800.0],
        transparent: true,
        icon_bytes: Some(std::fs::read(TruvisPath::resources_path("DruvisIII.png")).expect("failed to read icon file")),
    };
    StandaloneWinitHost::run(options, || Box::new(CornellRenderApp::default()));
}
