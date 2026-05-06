use truvis_app::outer_app::triangle::triangle_app::HelloTriangleApp;
use truvis_frame_runtime::RenderAppShell;
use truvis_winit_app::app::WinitApp;

fn main() {
    WinitApp::run_app(|| Box::new(RenderAppShell::new(HelloTriangleApp::default())));
}
