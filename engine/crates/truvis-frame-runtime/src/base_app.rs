use std::ffi::CStr;

use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use truvis_frame_api::frame_app::FrameAppHooks;
use truvis_frame_api::input_event::InputEvent;
use truvis_gfx::gfx::Gfx;
use truvis_logs::init_log;
use truvis_renderer::renderer::{Renderer, RendererInitCtx, RendererResizeCtx};

/// Invariant frame skeleton shared by concrete apps.
///
/// `BaseApp` owns only renderer infrastructure and the pending input event queue.
/// Camera, GUI, overlays, input state, and render-pipeline plugins belong to the
/// concrete app that implements `FrameAppHooks`.
pub struct BaseApp {
    renderer: Renderer,
    input_events: Vec<InputEvent>,
}

impl BaseApp {
    pub fn new(raw_display_handle: RawDisplayHandle) -> Self {
        let extra_instance_ext = ash_window::enumerate_required_extensions(raw_display_handle)
            .unwrap()
            .iter()
            .map(|ext| unsafe { CStr::from_ptr(*ext) })
            .collect();

        Self {
            renderer: Renderer::new(extra_instance_ext),
            input_events: Vec::new(),
        }
    }

    pub fn init_after_window(
        &mut self,
        raw_display_handle: RawDisplayHandle,
        raw_window_handle: RawWindowHandle,
        window_physical_size: [u32; 2],
    ) -> RendererInitCtx<'_> {
        self.renderer.init_after_window(raw_display_handle, raw_window_handle, window_physical_size)
    }

    pub fn push_input_event(&mut self, event: InputEvent) {
        self.input_events.push(event);
    }

    pub fn time_to_render(&self) -> bool {
        self.renderer.time_to_render()
    }

    pub fn recreate_swapchain_if_needed(&mut self, new_size: [u32; 2]) -> Option<RendererResizeCtx<'_>> {
        self.renderer.handle_resize(new_size)
    }

    pub fn run_frame(&mut self, app: &mut impl FrameAppHooks) {
        self.renderer.begin_frame();

        {
            let _span = tracy_client::span!("BaseApp::input");
            let input_events = std::mem::take(&mut self.input_events);
            app.on_input(&input_events);
        }

        {
            let _span = tracy_client::span!("BaseApp::update");
            let mut update_ctx = self.renderer.update_phase();
            app.update(&mut update_ctx);
        }

        self.renderer.prepare(app.camera());

        {
            let _span = tracy_client::span!("BaseApp::render");
            let render_ctx = self.renderer.render_phase();
            app.render(&render_ctx);
        }

        self.renderer.present();
        self.renderer.end_frame();
        tracy_client::frame_mark();
    }

    pub fn destroy(self) {
        Gfx::get().wait_idel();
        self.renderer.destroy();
        Gfx::destroy();
    }
}

pub fn init_env() {
    std::panic::set_hook(Box::new(|info| {
        log::error!("{}", info);
    }));
    init_log();
    tracy_client::Client::start();
}
