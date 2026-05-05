use truvis_app::outer_app::shader_toy::shader_toy_app::ShaderToy;
use truvis_frame_runtime::FrameAppShell;
use truvis_winit_app::app::WinitApp;

fn main() {
    WinitApp::run_app(|| Box::new(FrameAppShell::new(ShaderToy::default())));
}
