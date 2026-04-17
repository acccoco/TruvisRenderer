use truvis_app::outer_app::cornell_app::CornellApp;
use truvis_winit_app::app::WinitApp;

fn main() {
    WinitApp::run(|| Box::new(CornellApp::default()));
}
