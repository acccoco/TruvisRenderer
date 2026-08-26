use std::ffi::CStr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use raw_window_handle::{RawDisplayHandle, RawWindowHandle};

use truvis_render_runtime::render_runtime::RenderRuntime;

use crate::input_event::InputEvent;
use crate::render_thread_control::{RenderThreadControl, RenderThreadInit};
use crate::renderer::{Renderer, RendererInitCtx, RendererResizeCtx, RendererShutdownCtx};

/// 唯一合法的完整渲染生命周期与帧执行器。
///
/// `RenderLoop` 独占 `RenderRuntime`、待处理输入事件队列和一次性注入的具体 Renderer，
/// 同时统一驱动外层 loop 与 init、frame、resize、shutdown 顺序。具体 Renderer 仍自行
/// 持有 GUI、camera/input、overlay 和具体渲染子系统，并编排各阶段内部顺序。
pub struct RenderLoop {
    render_runtime: Option<RenderRuntime>,
    input_events: Vec<InputEvent>,
    renderer: Box<dyn Renderer>,
}

impl RenderLoop {
    fn new(renderer: Box<dyn Renderer>) -> Self {
        Self {
            render_runtime: None,
            input_events: Vec::new(),
            renderer,
        }
    }

    /// 在 OS RenderThread 内完成唯一一次初始化、固定帧循环和显式资源销毁。
    ///
    /// Renderer 必须由平台 factory 在当前线程内构造；窗口 owner 负责确保 raw handle
    /// 始终有效，直到本方法退出且 `RenderRuntime` 已销毁全部 Vulkan surface 资源。
    pub fn run(control: Arc<RenderThreadControl>, init: RenderThreadInit, renderer: Box<dyn Renderer>) {
        tracy_client::set_thread_name!("RenderThread");

        let mut render_loop = Self::new(renderer);
        render_loop.init_after_window(init.raw_display, init.raw_window, init.scale_factor, init.initial_size);

        let mut last_built_size = init.initial_size;
        let mut last_seen_resize_generation = control.resize_generation();
        let mut pending_resize_since: Option<Instant> = None;
        const RESIZE_DEBOUNCE: Duration = Duration::from_millis(80);

        while !control.exit_requested() {
            while let Some(event) = control.try_receive_input() {
                render_loop.push_input_event(event);
            }

            let resize_generation = control.resize_generation();
            if resize_generation != last_seen_resize_generation {
                last_seen_resize_generation = resize_generation;
                pending_resize_since = Some(Instant::now());
            }

            let [width, height] = control.latest_size();
            if width == 0 || height == 0 {
                std::thread::park_timeout(Duration::from_millis(1));
                continue;
            }

            if [width, height] != last_built_size && pending_resize_since.is_none() {
                pending_resize_since = Some(Instant::now());
            }

            if let Some(resize_since) = pending_resize_since {
                if resize_since.elapsed() < RESIZE_DEBOUNCE {
                    std::thread::park_timeout(Duration::from_millis(1));
                    continue;
                }

                render_loop.recreate_swapchain_if_needed([width, height]);
                last_built_size = [width, height];
                pending_resize_since = None;
            }

            if render_loop.has_pending_swapchain_recreate() {
                render_loop.recreate_swapchain_if_needed([width, height]);
                last_built_size = [width, height];
            }

            if !render_loop.time_to_render() {
                std::thread::park_timeout(Duration::from_millis(1));
                continue;
            }

            render_loop.run_frame();
        }

        log::info!("RenderThread: exit flag observed, destroying resources.");
        render_loop.shutdown();
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
            let mut renderer_ctx = RendererInitCtx {
                runtime,
                scale_factor,
                window_size,
            };
            self.renderer.init(&mut renderer_ctx);
        }
        self.render_runtime = Some(render_runtime);
    }

    fn run_frame(&mut self) {
        let _span = tracy_client::span!("RenderLoop::run_frame");
        let Self {
            render_runtime,
            input_events,
            renderer,
        } = self;
        let render_runtime = render_runtime.as_mut().expect("RenderRuntime missing in RenderLoop::run_frame");

        {
            let _span = tracy_client::span!("RenderLoop::begin_frame");
            render_runtime.begin_frame();
        }

        {
            let _span = tracy_client::span!("RenderLoop::input");
            let input_events = std::mem::take(input_events);
            renderer.on_input(&input_events);
        }

        {
            let _span = tracy_client::span!("RenderLoop::update");
            let mut update_ctx = render_runtime.update_phase();
            renderer.update(&mut update_ctx);
        }

        // DlssOptions 可能在 update/UI 阶段改变 DLSS SR mode。必须在 prepare/render graph 之前
        // 同步 render/output extent，并让 renderer-owned RT/GBuffer/DLSS targets 跟着重建。
        {
            let _span = tracy_client::span!("RenderLoop::sync_dlss_options_frame_state");
            if let Some(runtime) = render_runtime.sync_dlss_options_frame_state() {
                let image_extent = runtime.present.swapchain_image_info().image_extent;
                let new_size = [image_extent.width, image_extent.height];
                let mut renderer_ctx = RendererResizeCtx {
                    runtime,
                    window_size: new_size,
                };
                renderer.on_resize(&mut renderer_ctx);
            }
        }

        if !render_runtime.current_frame_has_present_target() {
            {
                let _span = tracy_client::span!("RenderLoop::skip_present_target");
                log::debug!("RenderLoop skips render/present because current frame has no acquired swapchain image.");
                render_runtime.signal_current_frame_complete();
            }
            {
                let _span = tracy_client::span!("RenderLoop::end_frame");
                render_runtime.end_frame();
            }
            tracy_client::frame_mark();
            return;
        }

        {
            let _span = tracy_client::span!("RenderLoop::prepare");
            render_runtime.prepare(&renderer.render_view());
        }
        {
            let _span = tracy_client::span!("RenderLoop::after_prepare");
            let mut ray_cast_ctx = render_runtime.ray_cast_phase();
            renderer.after_prepare(&mut ray_cast_ctx);
        }

        {
            let _span = tracy_client::span!("RenderLoop::render");
            let render_ctx = render_runtime.render_phase();
            renderer.render(&render_ctx);
        }

        {
            let _span = tracy_client::span!("RenderLoop::present");
            render_runtime.present();
        }
        {
            let _span = tracy_client::span!("RenderLoop::end_frame");
            render_runtime.end_frame();
        }
        tracy_client::frame_mark();
    }

    fn push_input_event(&mut self, event: InputEvent) {
        self.input_events.push(event);
    }

    fn recreate_swapchain_if_needed(&mut self, new_size: [u32; 2]) {
        let _span = tracy_client::span!("RenderLoop::recreate_swapchain_if_needed");
        let Self {
            render_runtime,
            renderer,
            ..
        } = self;
        let Some(runtime) = render_runtime
            .as_mut()
            .expect("RenderRuntime missing in RenderLoop::recreate_swapchain_if_needed")
            .handle_resize(new_size)
        else {
            return;
        };

        let mut renderer_ctx = RendererResizeCtx {
            runtime,
            window_size: new_size,
        };
        renderer.on_resize(&mut renderer_ctx);
    }

    fn time_to_render(&self) -> bool {
        self.render_runtime.as_ref().expect("RenderRuntime missing in RenderLoop::time_to_render").time_to_render()
    }

    fn has_pending_swapchain_recreate(&self) -> bool {
        self.render_runtime
            .as_ref()
            .expect("RenderRuntime missing in RenderLoop::has_pending_swapchain_recreate")
            .has_pending_swapchain_recreate()
    }

    fn shutdown(&mut self) {
        if let Some(render_runtime) = self.render_runtime.as_mut() {
            render_runtime.wait_idle();

            {
                let runtime = render_runtime.shutdown_phase();
                let mut renderer_ctx = RendererShutdownCtx { runtime };
                self.renderer.shutdown(&mut renderer_ctx);
            }
        }
        if let Some(render_runtime) = self.render_runtime.take() {
            Self::destroy_render_runtime(render_runtime);
        }
    }
}
