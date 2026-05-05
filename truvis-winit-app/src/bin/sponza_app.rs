use truvis_app::outer_app::sponza_app::SponzaApp;
use truvis_frame_runtime::FrameAppShell;
use truvis_winit_app::app::WinitApp;

fn main() {
    WinitApp::run_app(|| Box::new(FrameAppShell::new(SponzaApp::default())));
}
