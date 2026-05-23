use truvis_app_frame::RenderAppShell;
use truvis_sample_shader_toy::shader_toy_app::ShaderToy;
use truvis_winit_app::app::WinitApp;

fn main() {
    WinitApp::run_app(|| Box::new(RenderAppShell::new(ShaderToy::default())));
}
