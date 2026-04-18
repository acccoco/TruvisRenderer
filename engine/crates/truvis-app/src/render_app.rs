use crate::app_plugin::{AppPlugin, LegacyOuterAppAdapter};
use crate::gui_front::GuiHost;
#[allow(deprecated)]
use crate::outer_app::base::OuterApp;
use crate::overlay::{self, OverlayContext, OverlayModule};
use crate::platform::camera_controller::CameraController;
use crate::platform::input_manager::InputManager;
use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use std::ffi::CStr;
use truvis_gfx::gfx::Gfx;
use truvis_logs::init_log;
use truvis_renderer::renderer::Renderer;

pub fn panic_handler(info: &std::panic::PanicHookInfo) {
    log::error!("{}", info);
    // std::thread::sleep(std::time::Duration::from_secs(30));
}

/// 帧编排运行时：持有渲染后端、相机、输入、GUI 以及可插拔的应用逻辑，
/// 按阶段驱动每帧的完整生命周期。
///
/// 过渡期保留 [`RenderApp`] 类型别名以兼容旧调用方。
pub struct FrameRuntime {
    pub renderer: Renderer,
    pub camera_controller: CameraController,
    pub input_manager: InputManager,
    pub gui_host: GuiHost,

    pub last_render_area: ash::vk::Extent2D,

    pub plugin: Option<Box<dyn AppPlugin>>,

    /// Registrable overlay modules, rendered before `AppPlugin::build_ui`.
    /// Default overlays are registered in [`Self::new_with_plugin`].
    overlays: Vec<Box<dyn OverlayModule>>,
}
#[deprecated(note = "Renamed to `FrameRuntime`. This alias will be removed after the compatibility window.")]
pub type RenderApp = FrameRuntime;

// new & init
impl FrameRuntime {
    /// 使用 [`AppPlugin`] 构造 `FrameRuntime`。新代码应使用此入口。
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
            last_render_area: ash::vk::Extent2D::default(),
            overlays: overlay::default_overlays(),
        }
    }

    /// 兼容入口：接受旧 [`OuterApp`]，内部包装为 [`LegacyOuterAppAdapter`]。
    ///
    /// 现有 demo 在迁移到原生 `AppPlugin` 前使用此路径。
    #[deprecated(note = "Use `new_with_plugin`. All demos have been migrated to `AppPlugin`.")]
    #[allow(deprecated)]
    pub fn new(raw_display_handle: RawDisplayHandle, outer_app: Box<dyn OuterApp>) -> Self {
        Self::new_with_plugin(raw_display_handle, Box::new(LegacyOuterAppAdapter::new(outer_app)))
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
            self.plugin.as_mut().unwrap().init(&mut self.renderer, self.camera_controller.camera_mut());
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
// Overlay management
// ---------------------------------------------------------------------------
impl FrameRuntime {
    /// Append a custom overlay module. Overlays run in registration order,
    /// before `AppPlugin::build_ui`.
    pub fn add_overlay(&mut self, overlay: Box<dyn OverlayModule>) {
        self.overlays.push(overlay);
    }

    /// Remove all registered overlays (including defaults).
    pub fn clear_overlays(&mut self) {
        self.overlays.clear();
    }
}
// destroy
impl FrameRuntime {
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
// Swapchain resize (called by render_loop, outside of per-frame phases)
// ---------------------------------------------------------------------------
impl FrameRuntime {
    pub fn time_to_render(&mut self) -> bool {
        self.renderer.time_to_render()
    }

    /// 渲染线程轮询调用：统一处理 swapchain 重建判定并通知 plugin。
    ///
    /// 触发条件（任一成立）：
    /// 1) 传入的 `new_size` 与 `last_built_size` 不一致；
    /// 2) backend 报告 need_resize（例如 out-of-date/suboptimal）。
    ///
    /// 调用方负责保证 `new_size` 非零。
    /// resize / out-of-date 重建共享此单一入口，避免并行分叉流程。
    pub fn recreate_swapchain_if_needed(&mut self, new_size: [u32; 2], last_built_size: &mut [u32; 2]) {
        let size_changed = new_size != *last_built_size;
        if size_changed {
            log::debug!("swapchain rebuild: {:?} -> {:?}", *last_built_size, new_size);
            self.renderer.render_present.as_mut().unwrap().update_window_size(new_size);
        }

        if self.renderer.need_resize() {
            self.renderer.recreate_swapchain();
            self.plugin.as_mut().unwrap().on_resize(&mut self.renderer);
        }
        *last_built_size = new_size;
    }
}

// ---------------------------------------------------------------------------
// Frame phases
//
// 每帧执行顺序：
//   begin_frame → phase_input → phase_update → phase_prepare → phase_render → phase_present
//
// 不变量（invariants）：
//   1. 每个 phase 在单帧内至多执行一次。
//   2. resize / out-of-date 重建通过 `recreate_swapchain_if_needed` 单一入口触发，
//      不在 phase 内部发生 swapchain 重建。
//   3. 线程关闭握手由 render_loop 的 `shared.exit` 标志驱动；
//      phase 序列中不做关闭判定，`destroy()` 在循环退出后执行。
// ---------------------------------------------------------------------------
impl FrameRuntime {
    /// 兼容入口 —— 保持旧调用方（如 `render_loop`）可编译。
    ///
    /// 内部按 `begin_frame → phase_input → phase_update → phase_prepare
    /// → phase_render → phase_present` 顺序执行。
    pub fn big_update(&mut self) {
        if !self.time_to_render() {
            return;
        }
        self.run_frame();
    }

    /// 单帧完整执行：依次调用所有 phase。
    ///
    /// 调用方应保证 `time_to_render()` 已通过。
    pub fn run_frame(&mut self) {
        self.begin_frame();
        self.phase_input();
        self.phase_update();
        self.phase_prepare();
        self.phase_render();
        self.phase_present();
    }

    // -- begin_frame --------------------------------------------------------

    /// 帧起始：backend 初始化（timer / FIF wait / cmd reset / bindless）
    /// + AssetHub CPU 侧增量更新。
    ///
    /// **输入**: 无外部输入。
    /// **输出**: renderer 进入可录制新帧的状态，AssetHub 完成本帧 CPU 侧更新。
    ///
    /// Asset 更新由 FrameRuntime 显式调度（M3: Renderer 职责收敛）。
    fn begin_frame(&mut self) {
        let _span = tracy_client::span!("FrameRuntime::begin_frame");
        self.renderer.begin_frame();
        self.renderer.update_assets();
    }

    // -- phase_input --------------------------------------------------------

    /// 输入阶段：drain 事件通道，转发给 ImGui 与 InputManager。
    ///
    /// **输入**: `input_manager` 中缓存的平台事件。
    /// **输出**: `gui_host` 获得本帧 IO 状态，`input_manager` 完成 pressed/released 状态计算。
    fn phase_input(&mut self) {
        let _span = tracy_client::span!("FrameRuntime::phase_input");

        for event in self.input_manager.get_events() {
            // TODO: imgui 是否吞掉事件
            self.gui_host.handle_event(event);
        }

        self.input_manager.process_events();
    }

    // -- phase_update -------------------------------------------------------

    /// 更新阶段：同步 frame extent、acquire swapchain image、构建 GUI、
    /// 更新 camera 与 plugin CPU 逻辑。
    ///
    /// **输入**: 最新 swapchain extent、本帧 input state。
    /// **输出**: GUI draw data 已编译就绪、camera / plugin CPU 状态已推进。
    ///
    /// acquire_image 放在 CPU update 之前以简化 resize 处理（与历史行为一致）。
    fn phase_update(&mut self) {
        let _span = tracy_client::span!("FrameRuntime::phase_update");

        // 同步 frame settings（extent 等）
        self.renderer.update_frame_settings();

        // acquire swapchain image（与 resize 处理耦合，保持在 CPU update 前）
        self.renderer.acquire_image();

        // GUI: build → compile → prepare render data
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

        // camera + plugin CPU update
        {
            let _span = tracy_client::span!("FrameRuntime::phase_update::scene");
            let input_state = self.input_manager.state().clone();
            let frame_extent = self.renderer.render_context.frame_settings.frame_extent;

            self.camera_controller.update(
                &input_state,
                glam::vec2(frame_extent.width as f32, frame_extent.height as f32),
                self.renderer.timer.delta_time(),
            );

            self.plugin.as_mut().unwrap().update(&mut self.renderer);
        }
    }

    // -- phase_prepare ------------------------------------------------------

    /// 准备阶段：累积帧更新 + GPU 数据上传（gpu_scene、per-frame descriptors）。
    ///
    /// **输入**: camera 最终状态、scene_manager 已准备好的渲染数据。
    /// **输出**: GPU buffer 和 descriptor set 已更新，可供 render phase 使用。
    ///
    /// 累积帧跟踪由 FrameRuntime 显式调度（M3: Renderer 职责收敛），
    /// GPU 上传执行由 Renderer backend 负责。
    fn phase_prepare(&mut self) {
        let _span = tracy_client::span!("FrameRuntime::phase_prepare");
        self.renderer.update_accum_frames(self.camera_controller.camera());
        self.renderer.before_render(self.camera_controller.camera());
    }

    // -- phase_render -------------------------------------------------------

    /// 渲染阶段：plugin 录制 GPU 命令并提交。
    ///
    /// **输入**: renderer 处于 acquired 且 GPU 数据就绪状态；gui draw data 可用。
    /// **输出**: GPU 命令已提交（或排队），timeline semaphore 由 plugin 负责 signal。
    fn phase_render(&mut self) {
        let _span = tracy_client::span!("FrameRuntime::phase_render");

        self.plugin.as_mut().unwrap().render(
            &self.renderer,
            self.gui_host.get_render_data(),
            &self.renderer.fif_timeline_semaphore,
        );
    }

    // -- phase_present ------------------------------------------------------

    /// 呈现阶段：present swapchain image、推进帧计数器、Tracy frame mark。
    ///
    /// **输入**: render phase 已完成 GPU 提交。
    /// **输出**: 图像已提交到呈现引擎，frame counter 前进到下一帧。
    fn phase_present(&mut self) {
        let _span = tracy_client::span!("FrameRuntime::phase_present");

        self.renderer.present_image();
        self.renderer.end_frame();
        tracy_client::frame_mark();
    }
}

// ---------------------------------------------------------------------------
// Internal: overlay dispatch (M3: extracted from hardcoded UI to registrable modules)
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
