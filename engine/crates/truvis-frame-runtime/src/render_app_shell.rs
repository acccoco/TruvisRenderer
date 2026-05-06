use std::ffi::CStr;

use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use truvis_frame_api::input_event::InputEvent;
use truvis_frame_api::plugin::{PluginInitCtx, PluginResizeCtx, PluginShutdownCtx, PluginUpdateCtx};
use truvis_frame_api::render_app::{
    RenderApp, RenderAppHooks, RenderAppInitCtx, RenderAppResizeCtx, RenderAppShutdownCtx,
};
use truvis_render_backend::render_backend::RenderBackend;

/// 将具体 app hooks 转换为 render-loop [`RenderApp`] 的适配器。
///
/// `RenderAppShell` 持有 RenderBackend 和待处理输入事件队列，具体 app hooks
/// 持有 GUI、camera/input state、overlay 和 render plugin。
pub struct RenderAppShell<A> {
    render_backend: Option<RenderBackend>,
    input_events: Vec<InputEvent>,
    app: A,
}

impl<A> RenderAppShell<A> {
    pub fn new(app: A) -> Self {
        Self {
            render_backend: None,
            input_events: Vec::new(),
            app,
        }
    }

    pub fn app(&self) -> &A {
        &self.app
    }

    pub fn app_mut(&mut self) -> &mut A {
        &mut self.app
    }

    fn new_render_backend(raw_display_handle: RawDisplayHandle) -> RenderBackend {
        let extra_instance_ext = ash_window::enumerate_required_extensions(raw_display_handle)
            .unwrap()
            .iter()
            .map(|ext| unsafe { CStr::from_ptr(*ext) })
            .collect();

        RenderBackend::new(extra_instance_ext)
    }

    fn destroy_render_backend(render_backend: RenderBackend) {
        render_backend.destroy();
    }
}

impl<A> RenderApp for RenderAppShell<A>
where
    A: RenderAppHooks,
{
    fn init_after_window(
        &mut self,
        raw_display: RawDisplayHandle,
        raw_window: RawWindowHandle,
        scale_factor: f64,
        window_size: [u32; 2],
    ) {
        let mut render_backend = Self::new_render_backend(raw_display);
        {
            let backend = render_backend.init_after_window(raw_display, raw_window, window_size);
            let mut app_ctx = RenderAppInitCtx {
                backend,
                scale_factor,
                window_size,
            };
            self.app.init(&mut app_ctx);

            let RenderAppInitCtx { backend, .. } = app_ctx;
            let mut plugin_ctx = PluginInitCtx {
                device_ctx: backend.device_ctx,
                resource_ctx: backend.resource_ctx,
                queue_ctx: backend.queue_ctx,
                device_info_ctx: backend.device_info_ctx,
                immediate_ctx: backend.immediate_ctx,
                surface_ctx: backend.surface_ctx,
                world: backend.world,
                render_world: backend.render_world,
                cmd_allocator: backend.cmd_allocator,
                swapchain_image_info: backend.swapchain_image_info,
                render_present: backend.render_present,
            };
            self.app.visit_plugins_mut(&mut |plugin| {
                plugin.init(&mut plugin_ctx);
            });
        }
        self.render_backend = Some(render_backend);
    }

    fn run_frame(&mut self) {
        let Self {
            render_backend,
            input_events,
            app,
        } = self;
        let render_backend = render_backend.as_mut().expect("RenderBackend missing in RenderAppShell::run_frame");

        render_backend.begin_frame();

        {
            let _span = tracy_client::span!("RenderAppShell::input");
            let input_events = std::mem::take(input_events);
            app.on_input(&input_events);
        }

        {
            let _span = tracy_client::span!("RenderAppShell::update");
            let mut update_ctx = render_backend.update_phase();
            app.update(&mut update_ctx);

            let mut plugin_ctx = PluginUpdateCtx {
                world: update_ctx.world,
                pipeline_settings: update_ctx.pipeline_settings,
                frame_settings: update_ctx.frame_settings,
                delta_time_s: update_ctx.delta_time_s,
            };
            app.visit_plugins_mut(&mut |plugin| {
                plugin.update(&mut plugin_ctx);
            });
        }

        render_backend.prepare(app.camera());

        {
            let _span = tracy_client::span!("RenderAppShell::render");
            let render_ctx = render_backend.render_phase();
            app.render(&render_ctx);
        }

        render_backend.present();
        render_backend.end_frame();
        tracy_client::frame_mark();
    }

    fn push_input_event(&mut self, event: InputEvent) {
        self.input_events.push(event);
    }

    fn recreate_swapchain_if_needed(&mut self, new_size: [u32; 2]) {
        let Self {
            render_backend, app, ..
        } = self;
        let Some(backend) = render_backend
            .as_mut()
            .expect("RenderBackend missing in RenderAppShell::recreate_swapchain_if_needed")
            .handle_resize(new_size)
        else {
            return;
        };

        let mut app_ctx = RenderAppResizeCtx {
            backend,
            window_size: new_size,
        };
        app.on_resize(&mut app_ctx);

        let RenderAppResizeCtx { backend, .. } = app_ctx;
        let mut plugin_ctx = PluginResizeCtx {
            device_ctx: backend.device_ctx,
            resource_ctx: backend.resource_ctx,
            immediate_ctx: backend.immediate_ctx,
            surface_ctx: backend.surface_ctx,
            render_world: backend.render_world,
            render_present: backend.render_present,
        };
        app.visit_plugins_mut(&mut |plugin| {
            plugin.on_resize(&mut plugin_ctx);
        });
    }

    fn time_to_render(&self) -> bool {
        self.render_backend.as_ref().expect("RenderBackend missing in RenderAppShell::time_to_render").time_to_render()
    }

    fn shutdown(&mut self) {
        if let Some(render_backend) = self.render_backend.as_mut() {
            render_backend.wait_idle();

            {
                let backend = render_backend.shutdown_phase();
                let mut app_ctx = RenderAppShutdownCtx { backend };
                self.app.shutdown(&mut app_ctx);
            }
            {
                let backend = render_backend.shutdown_phase();
                let mut plugin_ctx = PluginShutdownCtx {
                    device_ctx: backend.device_ctx,
                    resource_ctx: backend.resource_ctx,
                    queue_ctx: backend.queue_ctx,
                    immediate_ctx: backend.immediate_ctx,
                    surface_ctx: backend.surface_ctx,
                    render_world: backend.render_world,
                    cmd_allocator: backend.cmd_allocator,
                };
                self.app.visit_plugins_mut_rev(&mut |plugin| {
                    plugin.shutdown(&mut plugin_ctx);
                });
            }
        }
        if let Some(render_backend) = self.render_backend.take() {
            Self::destroy_render_backend(render_backend);
        }
    }
}
