use std::ffi::CStr;

use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use truvis_render_runtime::render_runtime::RenderRuntime;

use crate::input_event::InputEvent;
use crate::plugin_api::{PluginInitCtx, PluginResizeCtx, PluginShutdownCtx, PluginUpdateCtx};
use crate::render_app_api::{RenderApp, RenderAppHooks, RenderAppInitCtx, RenderAppResizeCtx, RenderAppShutdownCtx};

/// 将具体 app hooks 转换为 render-loop [`RenderApp`] 的适配器。
///
/// `RenderAppShell` 持有 RenderRuntime 和待处理输入事件队列，具体 app hooks
/// 持有 GUI、camera/input state、overlay 和 render plugin。
pub struct RenderAppShell<A> {
    render_runtime: Option<RenderRuntime>,
    input_events: Vec<InputEvent>,
    app: A,
}

impl<A> RenderAppShell<A> {
    pub fn new(app: A) -> Self {
        Self {
            render_runtime: None,
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

    fn new_render_runtime(raw_display_handle: RawDisplayHandle) -> RenderRuntime {
        let extra_instance_ext = ash_window::enumerate_required_extensions(raw_display_handle)
            .unwrap()
            .iter()
            .map(|ext| unsafe { CStr::from_ptr(*ext) })
            .collect();

        RenderRuntime::new(extra_instance_ext)
    }

    fn destroy_render_runtime(render_runtime: RenderRuntime) {
        render_runtime.destroy();
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
        let mut render_runtime = Self::new_render_runtime(raw_display);
        {
            let runtime = render_runtime.init_after_window(raw_display, raw_window, window_size);
            let mut app_ctx = RenderAppInitCtx {
                runtime,
                scale_factor,
                window_size,
            };
            self.app.init(&mut app_ctx);

            let RenderAppInitCtx { runtime, .. } = app_ctx;
            let mut plugin_ctx = PluginInitCtx {
                device_ctx: runtime.device_ctx,
                resource_ctx: runtime.resource_ctx,
                queue_ctx: runtime.queue_ctx,
                device_info_ctx: runtime.device_info_ctx,
                immediate_ctx: runtime.immediate_ctx,
                surface_ctx: runtime.surface_ctx,
                world: runtime.world,
                gpu_store: runtime.gpu_store,
                cmd_allocator: runtime.cmd_allocator,
                swapchain_image_info: runtime.swapchain_image_info,
                present: runtime.present,
            };
            self.app.visit_plugins_mut(&mut |plugin| {
                plugin.init(&mut plugin_ctx);
            });
        }
        self.render_runtime = Some(render_runtime);
    }

    fn run_frame(&mut self) {
        let Self {
            render_runtime,
            input_events,
            app,
        } = self;
        let render_runtime = render_runtime.as_mut().expect("RenderRuntime missing in RenderAppShell::run_frame");

        render_runtime.begin_frame();

        {
            let _span = tracy_client::span!("RenderAppShell::input");
            let input_events = std::mem::take(input_events);
            app.on_input(&input_events);
        }

        {
            let _span = tracy_client::span!("RenderAppShell::update");
            let mut update_ctx = render_runtime.update_phase();
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

        if !render_runtime.current_frame_has_present_target() {
            log::debug!("RenderAppShell skips render/present because current frame has no acquired swapchain image.");
            render_runtime.signal_current_frame_complete();
            render_runtime.end_frame();
            tracy_client::frame_mark();
            return;
        }

        render_runtime.prepare(&app.render_view());
        {
            let _span = tracy_client::span!("RenderAppShell::after_prepare");
            let mut ray_cast_ctx = render_runtime.ray_cast_phase();
            app.after_prepare(&mut ray_cast_ctx);
        }

        {
            let _span = tracy_client::span!("RenderAppShell::render");
            let render_ctx = render_runtime.render_phase();
            app.render(&render_ctx);
        }

        render_runtime.present();
        render_runtime.end_frame();
        tracy_client::frame_mark();
    }

    fn push_input_event(&mut self, event: InputEvent) {
        self.input_events.push(event);
    }

    fn recreate_swapchain_if_needed(&mut self, new_size: [u32; 2]) {
        let Self {
            render_runtime, app, ..
        } = self;
        let Some(runtime) = render_runtime
            .as_mut()
            .expect("RenderRuntime missing in RenderAppShell::recreate_swapchain_if_needed")
            .handle_resize(new_size)
        else {
            return;
        };

        let mut app_ctx = RenderAppResizeCtx {
            runtime,
            window_size: new_size,
        };
        app.on_resize(&mut app_ctx);

        let RenderAppResizeCtx { runtime, .. } = app_ctx;
        let mut plugin_ctx = PluginResizeCtx {
            device_ctx: runtime.device_ctx,
            resource_ctx: runtime.resource_ctx,
            immediate_ctx: runtime.immediate_ctx,
            surface_ctx: runtime.surface_ctx,
            gpu_store: runtime.gpu_store,
            present: runtime.present,
        };
        app.visit_plugins_mut(&mut |plugin| {
            plugin.on_resize(&mut plugin_ctx);
        });
    }

    fn time_to_render(&self) -> bool {
        self.render_runtime.as_ref().expect("RenderRuntime missing in RenderAppShell::time_to_render").time_to_render()
    }

    fn has_pending_swapchain_recreate(&self) -> bool {
        self.render_runtime
            .as_ref()
            .expect("RenderRuntime missing in RenderAppShell::has_pending_swapchain_recreate")
            .has_pending_swapchain_recreate()
    }

    fn shutdown(&mut self) {
        if let Some(render_runtime) = self.render_runtime.as_mut() {
            render_runtime.wait_idle();

            {
                let runtime = render_runtime.shutdown_phase();
                let mut app_ctx = RenderAppShutdownCtx { runtime };
                self.app.shutdown(&mut app_ctx);
            }
            {
                let runtime = render_runtime.shutdown_phase();
                let mut plugin_ctx = PluginShutdownCtx {
                    device_ctx: runtime.device_ctx,
                    resource_ctx: runtime.resource_ctx,
                    queue_ctx: runtime.queue_ctx,
                    immediate_ctx: runtime.immediate_ctx,
                    surface_ctx: runtime.surface_ctx,
                    gpu_store: runtime.gpu_store,
                    cmd_allocator: runtime.cmd_allocator,
                };
                self.app.visit_plugins_mut_rev(&mut |plugin| {
                    plugin.shutdown(&mut plugin_ctx);
                });
            }
        }
        if let Some(render_runtime) = self.render_runtime.take() {
            Self::destroy_render_runtime(render_runtime);
        }
    }
}
