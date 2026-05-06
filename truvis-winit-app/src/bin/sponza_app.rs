use truvis_app::outer_app::sponza_app::SponzaApp;
use truvis_frame_runtime::RenderAppShell;
use truvis_winit_app::app::WinitApp;

fn main() {
    WinitApp::run_app(|| Box::new(RenderAppShell::new(SponzaApp::default())));
}
