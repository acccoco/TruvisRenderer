use truvis_app::outer_app::triangle::triangle_app::HelloTriangleApp;
use truvis_winit_app::app::WinitApp;

fn main() {
    WinitApp::run(|| Box::new(HelloTriangleApp::default()));
}
