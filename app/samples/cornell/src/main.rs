use cornell::cornell_app::CornellApp;
use truvis_app_frame::RenderAppShell;
use truvis_winit_app::app::WinitApp;

fn main() {
    WinitApp::run_app(|| Box::new(RenderAppShell::new(CornellApp::default())));
}
