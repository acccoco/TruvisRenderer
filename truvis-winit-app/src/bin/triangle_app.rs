use truvis_app::outer_app::triangle::triangle_app::HelloTriangleApp;
use truvis_winit_app::app::WinitApp;

fn main() {
    WinitApp::run_plugin(|| Box::new(HelloTriangleApp::default()));
}
