use truvis_app::outer_app::shader_toy::shader_toy_app::ShaderToy;
use truvis_winit_app::app::WinitApp;

fn main() {
    WinitApp::run_plugin(|| Box::new(ShaderToy::default()));
}
