use hello_triangle::triangle_app::HelloTriangleApp;
use truvis_winit_app::app::WinitApp;

fn main() {
    WinitApp::run_app(|| Box::new(HelloTriangleApp::default()));
}
