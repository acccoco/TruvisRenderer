use truvis_app_frame::RenderAppShell;
use truvis_sample_triangle::triangle_app::HelloTriangleApp;
use truvis_winit_app::app::WinitApp;

fn main() {
    WinitApp::run_app(|| Box::new(RenderAppShell::new(HelloTriangleApp::default())));
}
