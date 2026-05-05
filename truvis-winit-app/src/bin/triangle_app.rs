use truvis_app::outer_app::triangle::triangle_app::HelloTriangleApp;
use truvis_frame_runtime::FrameAppShell;
use truvis_winit_app::app::WinitApp;

fn main() {
    WinitApp::run_app(|| Box::new(FrameAppShell::new(HelloTriangleApp::default())));
}
