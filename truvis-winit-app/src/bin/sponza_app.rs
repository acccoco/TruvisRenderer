use truvis_app::outer_app::sponza_app::SponzaApp;
use truvis_winit_app::app::WinitApp;

fn main() {
    WinitApp::run(|| Box::new(SponzaApp::default()));
}
