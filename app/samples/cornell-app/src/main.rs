mod client;

use anyhow::Result;

use truvis_logs::LogFilePath;
use truvis_path::TruvisPath;
use truvis_render_loop::init_env_with_log_file;
use truvis_renderer::TruvisRenderer;
use truvis_scenes::StartupOptions;
use truvis_winit_host::{StandaloneWindowOptions, StandaloneWinitHost};

use crate::client::CornellAppClient;

struct CornellApp;

impl CornellApp {
    fn run() -> Result<()> {
        let startup_options = StartupOptions::from_process_args("rt-cornell")?;
        if startup_options.show_help {
            println!("{}", StartupOptions::usage("rt-cornell"));
            return Ok(());
        }

        init_env_with_log_file(LogFilePath::current_exe(TruvisPath::temp_dir()));
        let options = StandaloneWindowOptions {
            title: "Truvis".to_string(),
            logical_size: [1200.0, 800.0],
            transparent: true,
            icon_bytes: Some(std::fs::read(TruvisPath::resources_path("DruvisIII.png"))?),
        };
        // factory 在 RenderThread 内执行，Client 和 Renderer 的构造均留在该线程。
        StandaloneWinitHost::run(options, move || {
            let client = CornellAppClient::new(startup_options.scene);
            Box::new(TruvisRenderer::new(Box::new(client)))
        });
        Ok(())
    }
}

fn main() {
    if let Err(error) = CornellApp::run() {
        eprintln!("failed to run Truvis standalone: {error}");
        std::process::exit(1);
    }
}
