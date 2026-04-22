use std::ffi::CStr;

use raw_window_handle::{RawDisplayHandle, RawWindowHandle};

use truvis_app_api::app_plugin::{AppPlugin, InitCtx, RenderCtx, ResizeCtx, UpdateCtx};
use truvis_app_api::input_event::InputEvent;
use truvis_app_api::overlay::{self, OverlayContext, OverlayModule};
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
/// External callers drive frame advancement through the public API only:
///
/// - [`push_input_event`](Self::push_input_event)
/// - [`time_to_render`](Self::time_to_render)
/// - [`recreate_swapchain_if_needed`](Self::recreate_swapchain_if_needed)
/// - [`run_frame`](Self::run_frame)
/// - [`destroy`](Self::destroy)
pub struct FrameRuntime {
    renderer: Renderer,
    camera_controller: CameraController,
    input_manager: InputManager,
    gui_host: GuiHost,

    plugin: Option<Box<dyn AppPlugin>>,
    overlays: Vec<Box<dyn OverlayModule>>,
}

// ---------------------------------------------------------------------------
// Construction & initialization
// ---------------------------------------------------------------------------
impl FrameRuntime {
    pub fn new_with_plugin(raw_display_handle: RawDisplayHandle, plugin: Box<dyn AppPlugin>) -> Self {
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

        self.renderer.init_after_window(raw_display_handle, raw_window_handle, window_physical_size);

        {
            let _span = tracy_client::span!("AppPlugin::init");
            let mut ctx = InitCtx {
                camera: self.camera_controller.camera_mut(),
                swapchain_image_info: self.renderer.swapchain_image_info(),
                global_descriptor_sets: &self.renderer.render_context.global_descriptor_sets,
                render_present: self.renderer.render_present.as_ref().unwrap(),
                cmd_allocator: &mut self.renderer.cmd_allocator,
                scene_manager: &mut self.renderer.render_context.scene_manager,
                asset_hub: &mut self.renderer.render_context.asset_hub,
                gfx_resource_manager: &mut self.renderer.render_context.gfx_resource_manager,
                bindless_manager: &mut self.renderer.render_context.bindless_manager,
            };
            self.plugin.as_mut().unwrap().init(&mut ctx);
        };

        let (fonts_atlas, font_tex_id) = self.gui_host.init_font();
        self.renderer.render_present.as_mut().unwrap().gui_backend.register_font(
            &mut self.renderer.render_context.bindless_manager,
            &mut self.renderer.render_context.gfx_resource_manager,
            fonts_atlas,
            font_tex_id,
        );
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

    pub fn time_to_render(&mut self) -> bool {
        self.renderer.time_to_render()
    }

    pub fn recreate_swapchain_if_needed(&mut self, new_size: [u32; 2], last_built_size: &mut [u32; 2]) {
        let size_changed = new_size != *last_built_size;
        if size_changed {
            log::debug!("swapchain rebuild: {:?} -> {:?}", *last_built_size, new_size);
            self.renderer.render_present.as_mut().unwrap().update_window_size(new_size);
        }

        if self.renderer.need_resize() {
            self.renderer.recreate_swapchain();

            let mut ctx = ResizeCtx {
                frame_settings: &self.renderer.render_context.frame_settings,
                render_present: self.renderer.render_present.as_ref().unwrap(),
                global_descriptor_sets: &self.renderer.render_context.global_descriptor_sets,
                gfx_resource_manager: &mut self.renderer.render_context.gfx_resource_manager,
                bindless_manager: &mut self.renderer.render_context.bindless_manager,
            };
            self.plugin.as_mut().unwrap().on_resize(&mut ctx);
        }
        *last_built_size = new_size;
    }

    pub fn run_frame(&mut self) {
        self.begin_frame();
        self.phase_input();
        self.phase_update();
        self.phase_prepare();
        self.phase_render();
        self.phase_present();
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
// Frame phases (internal)
// ---------------------------------------------------------------------------
impl FrameRuntime {
    fn begin_frame(&mut self) {
        let _span = tracy_client::span!("FrameRuntime::begin_frame");
        self.renderer.begin_frame();
        self.renderer.update_assets();
    }

    fn phase_input(&mut self) {
        let _span = tracy_client::span!("FrameRuntime::phase_input");

        for event in self.input_manager.get_events() {
            self.gui_host.handle_event(event);
        }

        self.input_manager.process_events();
    }

    fn phase_update(&mut self) {
        let _span = tracy_client::span!("FrameRuntime::phase_update");

        self.renderer.update_frame_settings();
        self.renderer.acquire_image();

        {
            let _span = tracy_client::span!("FrameRuntime::phase_update::build_ui");
            self.build_ui();
            self.gui_host.compile_ui();

            let frame_label = self.renderer.frame_label();
            self.renderer
                .render_present
                .as_mut()
                .unwrap()
                .gui_backend
                .prepare_render_data(self.gui_host.get_render_data(), frame_label);
        }

        {
            let _span = tracy_client::span!("FrameRuntime::phase_update::scene");
            let input_state = self.input_manager.state().clone();
            let frame_extent = self.renderer.render_context.frame_settings.frame_extent;

            self.camera_controller.update(
                &input_state,
                glam::vec2(frame_extent.width as f32, frame_extent.height as f32),
                self.renderer.timer.delta_time(),
            );

            let mut ctx = UpdateCtx {
                scene_manager: &mut self.renderer.render_context.scene_manager,
                pipeline_settings: &mut self.renderer.render_context.pipeline_settings,
                frame_settings: &self.renderer.render_context.frame_settings,
                delta_time_s: self.renderer.render_context.delta_time_s,
            };
            self.plugin.as_mut().unwrap().update(&mut ctx);
        }
    }

    fn phase_prepare(&mut self) {
        let _span = tracy_client::span!("FrameRuntime::phase_prepare");
        self.renderer.update_accum_frames(self.camera_controller.camera());
        self.renderer.before_render(self.camera_controller.camera());
    }

    fn phase_render(&mut self) {
        let _span = tracy_client::span!("FrameRuntime::phase_render");

        let gui_draw_data = self.gui_host.get_render_data();
        let ctx = RenderCtx {
            render_context: &self.renderer.render_context,
            render_present: self.renderer.render_present.as_ref().unwrap(),
            gui_draw_data,
            timeline: &self.renderer.fif_timeline_semaphore,
        };
        self.plugin.as_ref().unwrap().render(&ctx);
    }

    fn phase_present(&mut self) {
        let _span = tracy_client::span!("FrameRuntime::phase_present");

        self.renderer.present_image();
        self.renderer.end_frame();
        tracy_client::frame_mark();
    }
}

// ---------------------------------------------------------------------------
// Internal: overlay dispatch
// ---------------------------------------------------------------------------
impl FrameRuntime {
    fn build_ui(&mut self) {
        let elapsed = self.renderer.timer.delta_time();
        let swapchain_extent = self.renderer.render_present.as_ref().unwrap().swapchain.as_ref().unwrap().extent();
        let accum_frames_num = self.renderer.render_context.accum_data.accum_frames_num();

        let camera = self.camera_controller.camera();
        let pipeline_settings = &mut self.renderer.render_context.pipeline_settings;
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
}
