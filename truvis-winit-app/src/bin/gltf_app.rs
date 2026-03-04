use truvis_app::outer_app::gltf_app::GltfApp;
use truvis_winit_app::app::WinitApp;

fn main() {
    let outer_app = Box::new(GltfApp::default());
    WinitApp::run(outer_app);
}
