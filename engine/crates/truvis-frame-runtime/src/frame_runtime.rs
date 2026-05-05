use std::ffi::CStr;

use raw_window_handle::{RawDisplayHandle, RawWindowHandle};

use truvis_frame_api::frame_plugin::{FramePlugin, RenderCtx, UpdateCtx};
use truvis_frame_api::input_event::InputEvent;
use truvis_frame_api::overlay::{self, OverlayContext, OverlayModule};
use truvis_gfx::gfx::Gfx;
use truvis_logs::init_log;
use truvis_renderer::renderer::Renderer;

use crate::camera_controller::CameraController;
use crate::gui_front::GuiHost;
use crate::input_manager::InputManager;

pub fn panic_handler(info: &std::panic::PanicHookInfo) {
    log::error!("{}", info);
}

/// Frame orchestration runtime.
///
/// Drives the per-frame lifecycle by calling Renderer lifecycle methods
/// and connecting the returned Ctx to Plugin hooks. Does NOT access
/// Renderer internal fields directly.
pub struct FrameRuntime {
    renderer: Renderer,
    camera_controller: CameraController,
    input_manager: InputManager,
    gui_host: GuiHost,

    plugin: Option<Box<dyn FramePlugin>>,
    overlays: Vec<Box<dyn OverlayModule>>,
}

// ---------------------------------------------------------------------------
// Construction & initialization
// ---------------------------------------------------------------------------
impl FrameRuntime {
    pub fn new_with_plugin(raw_display_handle: RawDisplayHandle, plugin: Box<dyn FramePlugin>) -> Self {
        let extra_instance_ext = ash_window::enumerate_required_extensions(raw_display_handle)
            .unwrap()
            .iter()
            .map(|ext| unsafe { CStr::from_ptr(*ext) })
            .collect();

        let renderer = Renderer::new(extra_instance_ext);
        let camera_controller = CameraController::new();

        Self {
            renderer,
            plugin: Some(plugin),
            camera_controller,
            input_manager: InputManager::new(),
            gui_host: GuiHost::new(),
            overlays: overlay::default_overlays(),
        }
    }

    pub fn init_after_window(
        &mut self,
        raw_display_handle: RawDisplayHandle,
        raw_window_handle: RawWindowHandle,
        window_scale_factor: f64,
        window_physical_size: [u32; 2],
    ) {
        self.gui_host.hidpi_factor = window_scale_factor;

        {
            let mut ctx = self.renderer.init_after_window(raw_display_handle, raw_window_handle, window_physical_size);
            let _span = tracy_client::span!("FramePlugin::init");
            self.plugin.as_mut().unwrap().init(&mut ctx, self.camera_controller.camera_mut());
        }
        // ctx dropped — renderer unlocked

        let (fonts_atlas, font_tex_id) = self.gui_host.init_font();
        self.renderer.register_gui_font(fonts_atlas, font_tex_id);
    }

    pub fn init_env() {
        std::panic::set_hook(Box::new(panic_handler));
        init_log();
        tracy_client::Client::start();
    }
}

// ---------------------------------------------------------------------------
// Runtime single-entry API
// ---------------------------------------------------------------------------
impl FrameRuntime {
    pub fn push_input_event(&mut self, event: InputEvent) {
        self.input_manager.push_event(event);
    }

    pub fn time_to_render(&self) -> bool {
        self.renderer.time_to_render()
    }

    pub fn recreate_swapchain_if_needed(&mut self, new_size: [u32; 2], last_built_size: &mut [u32; 2]) {
        if new_size == *last_built_size {
            return;
        }

        log::debug!("swapchain rebuild: {:?} -> {:?}", *last_built_size, new_size);

        if let Some(mut ctx) = self.renderer.handle_resize(new_size) {
            self.plugin.as_mut().unwrap().on_resize(&mut ctx);
        }

        *last_built_size = new_size;
    }

    pub fn run_frame(&mut self) {
        // 1. begin_frame (self-contained: timer, FIF wait, resource cleanup, assets)
        self.renderer.begin_frame();

        // 2. process input
        self.phase_input();

        // 3. update_phase → Ctx for build_ui + plugin.update
        let (swapchain_extent, delta_time) = {
            let _span = tracy_client::span!("FrameRuntime::phase_update");
            let update_ctx = self.renderer.update_phase();

            // build_ui: pass Ctx data to overlay + plugin UI (reborrow pipeline_settings)
            {
                let _span = tracy_client::span!("FrameRuntime::build_ui");
                let elapsed = std::time::Duration::from_secs_f32(update_ctx.delta_time_s);
                let swapchain_extent = update_ctx.swapchain_extent;
                let accum_frames_num = update_ctx.accum_data.accum_frames_num();
                let camera = self.camera_controller.camera();
                let pipeline_settings = &mut *update_ctx.pipeline_settings;
                let plugin = self.plugin.as_mut().unwrap();
                let overlays = &mut self.overlays;

                self.gui_host.new_frame(elapsed, |ui| {
                    let mut ctx = OverlayContext {
                        delta_time_s: elapsed.as_secs_f32(),
                        swapchain_extent,
                        camera,
                        accum_frames_num,
                        pipeline_settings,
                    };
                    for overlay in overlays.iter_mut() {
                        overlay.build_ui(ui, &mut ctx);
                    }
                    plugin.build_ui(ui);
                });
            }

            // plugin.update (reborrow again from update_ctx)
            let mut plugin_ctx = UpdateCtx {
                world: &mut *update_ctx.world,
                pipeline_settings: &mut *update_ctx.pipeline_settings,
                frame_settings: update_ctx.frame_settings,
                delta_time_s: update_ctx.delta_time_s,
            };
            self.plugin.as_mut().unwrap().update(&mut plugin_ctx);

            (update_ctx.swapchain_extent, update_ctx.delta_time_s)
        };
        // update_ctx dropped here — Renderer unlocked

        // 4. compile UI + submit GUI data
        self.gui_host.compile_ui();
        self.renderer.submit_gui_data(self.gui_host.get_render_data());

        // 5. camera update (uses swapchain_extent + delta_time saved from Ctx)
        {
            let input_state = self.input_manager.state().clone();
            self.camera_controller.update(
                &input_state,
                glam::vec2(swapchain_extent.width as f32, swapchain_extent.height as f32),
                std::time::Duration::from_secs_f32(delta_time),
            );
        }

        // 6. prepare (accum frames + GPU scene upload)
        self.renderer.prepare(self.camera_controller.camera());

        // 7. render_phase → compose RenderCtx → plugin.render
        {
            let _span = tracy_client::span!("FrameRuntime::phase_render");
            let renderer_ctx = self.renderer.render_phase();
            let gui_draw_data = self.gui_host.get_render_data();
            let ctx = RenderCtx {
                render_world: renderer_ctx.render_world,
                render_present: renderer_ctx.render_present,
                gui_draw_data,
                timeline: renderer_ctx.timeline,
            };
            self.plugin.as_ref().unwrap().render(&ctx);
        }

        // 8. present + end_frame
        self.renderer.present();
        self.renderer.end_frame();
        tracy_client::frame_mark();
    }

    pub fn destroy(mut self) {
        Gfx::get().wait_idel();

        if let Some(plugin) = self.plugin.as_mut() {
            plugin.shutdown();
        }
        self.plugin = None;
        self.renderer.destroy();

        Gfx::destroy();
    }
}

// ---------------------------------------------------------------------------
// Overlay management
// ---------------------------------------------------------------------------
impl FrameRuntime {
    pub fn add_overlay(&mut self, overlay: Box<dyn OverlayModule>) {
        self.overlays.push(overlay);
    }

    pub fn clear_overlays(&mut self) {
        self.overlays.clear();
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------
impl FrameRuntime {
    fn phase_input(&mut self) {
        let _span = tracy_client::span!("FrameRuntime::phase_input");

        for event in self.input_manager.get_events() {
            self.gui_host.handle_event(event);
        }

        self.input_manager.process_events();
    }
}
