use truvis_render_foundation::render_view::RenderView;
use truvis_render_graph::render_graph::{RenderGraphBuilder, RgSemaphoreInfo};
use truvis_render_loop::input_event::InputEvent;
use truvis_render_loop::renderer::{Renderer, RendererInitCtx, RendererResizeCtx, RendererShutdownCtx};
use truvis_render_runtime::ray_cast::{RayCastRay, RayCastResult};
use truvis_render_runtime::render_runtime::{RenderRuntimeRayCastCtx, RenderRuntimeRenderCtx, RenderRuntimeUpdateCtx};
use truvis_render_runtime::selection::WorldSubmeshSelection;
use truvis_world::GameWorld;

use renderer_imgui::{FrameStatsOverlayData, ImGuiSubsystem};
use renderer_kit::camera_controller::CameraController;
use renderer_kit::debug_image::DebugImageSelection;
use renderer_kit::input_state::InputManager;
use renderer_kit::subsystem::{SubsystemLifecycle, SubsystemRenderCtx};
use renderer_rendering::{OfflineRenderSubsystem, PathTracingCommonSettings, RealtimeRenderSubsystem, RenderMode};

use crate::TruvisRendererPorts;
use crate::coordinate_gizmo::CoordinateGizmoSubsystem;
use crate::desktop_command::DesktopCommandController;
use crate::editor_controller::{EditorController, EditorControllerConfig};
use crate::overlay_ui::{
    DebugImageSelectionData, RaycastOverlayData, RenderControlsData, TruvisOverlayFrame, TruvisOverlayOptions,
    TruvisOverlayUi,
};
use crate::scenes;
use crate::selection_outline::SelectionOutlineSubsystem;

pub struct TruvisRenderer {
    imgui: ImGuiSubsystem,
    debug_image_selection: DebugImageSelection,
    realtime: RealtimeRenderSubsystem,
    offline: OfflineRenderSubsystem,
    selection_outline: SelectionOutlineSubsystem,
    coordinate_gizmo: CoordinateGizmoSubsystem,
    path_tracing_common_settings: PathTracingCommonSettings,
    render_mode: RenderMode,
    camera_controller: CameraController,
    input: InputManager,
    overlay_ui: TruvisOverlayUi,
    click_ray_cast_probe: ClickRayCastProbe,
    selected_submesh: Option<WorldSubmeshSelection>,

    /// RenderThread 独占的 Tauri 桌面特权命令消费者。
    ///
    /// 它只在 update 阶段短暂借用 `GameWorld`，确保文件选择结果不会让 Tauri 主线程、
    /// WebView 或 Tauri IPC owner 越过 CPU scene 权威边界。
    desktop_command_controller: DesktopCommandController,

    editor_controller: EditorController,
    scene_initializer: scenes::SceneInitializer,
}

impl TruvisRenderer {
    /// 使用 frontend 壳预先创建的 [`TruvisRendererPorts`] 构造渲染侧业务状态。
    ///
    /// Editor IPC 生命周期属于 Tauri desktop；本 Renderer 只拥有 Editor 协议和桌面特权
    /// command 到权威 `GameWorld` 的非阻塞 controller，避免 RenderThread 同时承担窗口壳和
    /// 网络 owner 职责。
    pub fn new(ports: TruvisRendererPorts, initial_scene: scenes::InitialScene) -> Self {
        Self {
            imgui: Default::default(),
            debug_image_selection: Default::default(),
            realtime: Default::default(),
            offline: Default::default(),
            selection_outline: Default::default(),
            coordinate_gizmo: Default::default(),
            path_tracing_common_settings: Default::default(),
            render_mode: Default::default(),
            camera_controller: Default::default(),
            input: Default::default(),
            overlay_ui: Default::default(),
            click_ray_cast_probe: Default::default(),
            selected_submesh: None,
            desktop_command_controller: ports.desktop_commands,
            editor_controller: EditorController::new(ports.editor, EditorControllerConfig::default()),
            scene_initializer: scenes::SceneInitializer::new(initial_scene),
        }
    }
}

pub(crate) struct ClickRayCastProbe {
    total_time_s: f32,
    pending_ray: Option<RayCastRay>,
    pending_screen_pos: Option<glam::Vec2>,
    last_screen_pos: Option<glam::Vec2>,
    last_result: Option<RayCastResult>,
    last_error: Option<String>,
    last_cast_time_s: Option<f32>,
}

impl Default for ClickRayCastProbe {
    fn default() -> Self {
        Self {
            total_time_s: 0.0,
            pending_ray: None,
            pending_screen_pos: None,
            last_screen_pos: None,
            last_result: None,
            last_error: None,
            last_cast_time_s: None,
        }
    }
}

impl ClickRayCastProbe {
    fn update_time(&mut self, delta_time_s: f32) {
        self.total_time_s += delta_time_s.max(0.0);
    }

    fn request_cast(&mut self, screen_pos: glam::Vec2, ray: Option<RayCastRay>) {
        self.last_screen_pos = Some(screen_pos);
        match ray {
            Some(ray) => {
                self.pending_ray = Some(ray);
                self.pending_screen_pos = Some(screen_pos);
                self.last_error = None;
            }
            None => {
                self.pending_ray = None;
                self.pending_screen_pos = None;
                self.last_result = None;
                self.last_error = Some("click position is outside the viewport".to_owned());
                self.last_cast_time_s = None;
            }
        }
    }

    fn take_pending_cast(&mut self) -> Option<(RayCastRay, glam::Vec2)> {
        let ray = self.pending_ray.take()?;
        let screen_pos = self.pending_screen_pos.take().expect("pending raycast missing screen position");
        Some((ray, screen_pos))
    }

    fn finish_cast(&mut self, screen_pos: glam::Vec2, result: Result<RayCastResult, String>) {
        match result {
            Ok(result) => {
                self.last_result = Some(result);
                self.last_error = None;
            }
            Err(err) => {
                self.last_result = None;
                self.last_error = Some(err);
            }
        }
        self.last_screen_pos = Some(screen_pos);
        self.last_cast_time_s = Some(self.total_time_s);
    }

    pub(crate) fn has_pending_cast(&self) -> bool {
        self.pending_ray.is_some()
    }

    pub(crate) fn last_screen_pos(&self) -> Option<glam::Vec2> {
        self.last_screen_pos
    }

    pub(crate) fn last_cast_time_s(&self) -> Option<f32> {
        self.last_cast_time_s
    }

    pub(crate) fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    pub(crate) fn last_result(&self) -> Option<&RayCastResult> {
        self.last_result.as_ref()
    }
}

impl TruvisRenderer {
    pub fn overlay_options(&self) -> &TruvisOverlayOptions {
        self.overlay_ui.options()
    }

    pub fn overlay_options_mut(&mut self) -> &mut TruvisOverlayOptions {
        self.overlay_ui.options_mut()
    }

    fn cast_single_ray(ctx: &mut RenderRuntimeRayCastCtx<'_>, ray: RayCastRay) -> Result<RayCastResult, String> {
        ctx.cast_sync(std::slice::from_ref(&ray))
            .map_err(|err| err.to_string())
            .map(|mut results| results.pop().expect("single raycast result missing"))
    }

    fn selection_from_raycast_result(result: &Result<RayCastResult, String>) -> Option<WorldSubmeshSelection> {
        match result {
            Ok(RayCastResult::Hit(hit)) => Some(WorldSubmeshSelection {
                instance: hit.instance,
                submesh_index: hit.submesh_index,
            }),
            Ok(RayCastResult::Miss) | Err(_) => None,
        }
    }

    fn clear_stale_selection(&mut self, world: &GameWorld) -> bool {
        let Some(selection) = self.selected_submesh else {
            return false;
        };

        let scene = world.scene_view();
        let valid = scene
            .get_instance(selection.instance)
            .is_some_and(|instance| (selection.submesh_index as usize) < instance.materials.len());
        if !valid {
            // 这里只清理 CPU 语义已失效的选择；GPU 未 ready / pending 由 runtime resolver 在
            // render 阶段返回“不绘制”，避免 update 阶段感知 RenderWorld 内部状态。
            self.selected_submesh = None;
            return true;
        }
        false
    }
}

impl Renderer for TruvisRenderer {
    fn init(&mut self, ctx: &mut RendererInitCtx<'_>) {
        self.render_mode = RenderMode::initial_from_env();
        self.imgui.set_hidpi_factor(ctx.scale_factor);
        self.imgui.set_display_size(ctx.window_size);

        self.scene_initializer.initialize(&mut *ctx.runtime.world, self.camera_controller.camera_mut());

        // Renderer 持有初始化顺序：场景 CPU 状态先就绪，再依次创建具体渲染资源。
        self.realtime.init(&mut ctx.runtime);
        self.offline.init(&mut ctx.runtime);
        self.selection_outline.init(&mut ctx.runtime);
        self.coordinate_gizmo.init(&mut ctx.runtime);
        self.imgui.init(&mut ctx.runtime);
    }

    fn on_input(&mut self, events: &[InputEvent]) {
        self.input.begin_frame();
        for event in events {
            if !self.imgui.on_input(event) {
                self.input.process_event(event);
            }
        }
    }

    fn update(&mut self, ctx: &mut RenderRuntimeUpdateCtx) {
        self.scene_initializer.update(ctx.world);
        self.click_ray_cast_probe.update_time(ctx.frame_timing.delta_time_s());
        if self.clear_stale_selection(ctx.world) {
            self.editor_controller.notify_selection_changed(None);
        }
        let desktop_update = self.desktop_command_controller.process_next(ctx.world);
        if let Some(scene_version) = desktop_update.scene_version_changed {
            self.editor_controller.notify_scene_version_changed(scene_version);
        }
        self.editor_controller.process_requests(ctx.world, self.selected_submesh);

        let delta = std::time::Duration::from_secs_f32(ctx.frame_timing.delta_time_s());
        let viewport_size = glam::vec2(ctx.swapchain_extent.width as f32, ctx.swapchain_extent.height as f32);
        self.camera_controller.update_with_wheel_zoom(self.input.state(), viewport_size, delta);

        if self.input.state().is_left_button_just_pressed() {
            let mouse_position = self.input.state().mouse_position();
            let screen_pos = glam::vec2(mouse_position[0] as f32, mouse_position[1] as f32);
            let ray = self.camera_controller.make_screen_raycast(mouse_position, viewport_size);
            if ray.is_none() {
                if self.selected_submesh.take().is_some() {
                    self.editor_controller.notify_selection_changed(None);
                }
            }
            self.click_ray_cast_probe.request_cast(screen_pos, ray);
        }

        self.imgui.build_frame(delta, |ui| {
            let offline_sample_count = self.offline.sample_count();
            let frame = TruvisOverlayFrame {
                ui,
                stats: FrameStatsOverlayData {
                    camera: self.camera_controller.camera(),
                    swapchain_extent: ctx.swapchain_extent,
                    accum_frames_num: ctx.view_accum.accum_frames_num(),
                },
                render_controls: RenderControlsData {
                    render_mode: &mut self.render_mode,
                    dlss_options: ctx.dlss_options,
                    common_settings: &mut self.path_tracing_common_settings,
                    realtime_settings: self.realtime.settings_mut(),
                    offline_settings: self.offline.settings_mut(),
                    offline_sample_count,
                },
                raycast: RaycastOverlayData {
                    probe: &self.click_ray_cast_probe,
                    world: ctx.world,
                },
                debug_images: DebugImageSelectionData {
                    selection: &mut self.debug_image_selection,
                    realtime_options: RealtimeRenderSubsystem::debug_image_options(),
                    offline_options: OfflineRenderSubsystem::debug_image_options(),
                },
            };
            self.overlay_ui.build(frame);

            let debug_image_options = match self.render_mode {
                RenderMode::Realtime => RealtimeRenderSubsystem::debug_image_options(),
                RenderMode::Offline => OfflineRenderSubsystem::debug_image_options(),
            };
            // 选择归一化属于 Renderer 状态维护，不能依赖 Debug Images window/section 当前是否可见。
            self.debug_image_selection.normalize_options(debug_image_options);
        });
    }

    fn after_prepare(&mut self, ctx: &mut RenderRuntimeRayCastCtx<'_>) {
        if let Some(request) = self.camera_controller.take_pending_pivot_raycast() {
            let result = Self::cast_single_ray(ctx, request.ray);
            self.camera_controller.finish_pivot_raycast(request, result);
        }

        if let Some(request) = self.camera_controller.take_pending_drag_pan_raycast() {
            let result = Self::cast_single_ray(ctx, request.ray);
            self.camera_controller.finish_drag_pan_raycast(request, result);
        }

        if let Some(request) = self.camera_controller.take_pending_wheel_zoom_raycast() {
            let result = Self::cast_single_ray(ctx, request.ray);
            self.camera_controller.finish_wheel_zoom_raycast(request, result);
        }

        if let Some((ray, screen_pos)) = self.click_ray_cast_probe.take_pending_cast() {
            let result = Self::cast_single_ray(ctx, ray);
            let selection = Self::selection_from_raycast_result(&result);
            if selection != self.selected_submesh {
                let editor_selection = match &result {
                    Ok(RayCastResult::Hit(hit)) => Some((hit.instance, hit.submesh_index, hit.material)),
                    Ok(RayCastResult::Miss) | Err(_) => None,
                };
                self.editor_controller.notify_selection_changed(editor_selection);
            }
            self.selected_submesh = selection;
            self.click_ray_cast_probe.finish_cast(screen_pos, result);
        }
    }

    fn on_resize(&mut self, ctx: &mut RendererResizeCtx<'_>) {
        self.realtime.on_resize(&mut ctx.runtime);
        self.offline.on_resize(&mut ctx.runtime);
        self.selection_outline.on_resize(&mut ctx.runtime);
        self.coordinate_gizmo.on_resize(&mut ctx.runtime);
        self.imgui.on_resize(&mut ctx.runtime);
    }

    fn shutdown(&mut self, ctx: &mut RendererShutdownCtx<'_>) {
        self.desktop_command_controller.shutdown();
        self.editor_controller.shutdown();

        // 与资源创建顺序相反释放，且始终早于 runtime root owner 销毁。
        self.imgui.shutdown(&mut ctx.runtime);
        self.coordinate_gizmo.shutdown(&mut ctx.runtime);
        self.selection_outline.shutdown(&mut ctx.runtime);
        self.offline.shutdown(&mut ctx.runtime);
        self.realtime.shutdown(&mut ctx.runtime);
    }

    fn render(&mut self, ctx: &RenderRuntimeRenderCtx) {
        let subsystem_ctx = SubsystemRenderCtx::from_runtime(ctx);
        let frame_label = ctx.record_ctx.frame_timing.frame_label();
        let frame_id = ctx.record_ctx.frame_timing.frame_id();

        // 离线累计失效由 Renderer 在每帧 render 前统一判断：相机、场景和离线设置都已经进入
        // 本帧确定状态，离线渲染子系统只保存历史签名并在变化时清空自己的 accum_image。
        self.offline.update_accum_signature(
            self.camera_controller.camera().render_view().accum_signature(),
            ctx.render_scene.accum_signature(frame_label),
            &self.path_tracing_common_settings,
        );

        self.imgui.prepare_render_data(&subsystem_ctx);
        let selected_debug_image_id = self.debug_image_selection.selected_id();

        // Renderer 持有实时/离线模式选择；具体渲染子系统只负责向 RenderGraph 贡献自己的 compute subgraph。
        // 两条分支都生成同一队列上的第一段 submit，保证后续 present graph 可按统一顺序消费结果。
        let compute_submit = match self.render_mode {
            RenderMode::Realtime => {
                let mut graph = RenderGraphBuilder::new();
                self.realtime.contribute_compute_passes(&mut graph, &subsystem_ctx, &self.path_tracing_common_settings);
                let compiled_graph = graph.compile();
                if log::log_enabled!(log::Level::Debug) {
                    static PRINT_RT_COMPUTE_DEBUG_INFO: std::sync::Once = std::sync::Once::new();
                    PRINT_RT_COMPUTE_DEBUG_INFO.call_once(|| {
                        compiled_graph.print_execution_plan();
                    });
                }

                let cmd = self.realtime.compute_cmd(frame_label);
                cmd.begin(ash::vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT, "rt-compute-graph");
                compiled_graph.execute(cmd, ctx.record_ctx.gfx_resource_registry);
                cmd.end();
                compiled_graph.build_submit_info(std::slice::from_ref(cmd))
            }
            RenderMode::Offline => {
                let mut graph = RenderGraphBuilder::new();
                self.offline.contribute_compute_passes(&mut graph, &subsystem_ctx, &self.path_tracing_common_settings);
                let compiled_graph = graph.compile();
                if log::log_enabled!(log::Level::Debug) {
                    static PRINT_OFFLINE_COMPUTE_DEBUG_INFO: std::sync::Once = std::sync::Once::new();
                    PRINT_OFFLINE_COMPUTE_DEBUG_INFO.call_once(|| {
                        compiled_graph.print_execution_plan();
                    });
                }

                let cmd = self.offline.compute_cmd(frame_label);
                cmd.begin(ash::vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT, "offline-compute-graph");
                compiled_graph.execute(cmd, ctx.record_ctx.gfx_resource_registry);
                cmd.end();
                compiled_graph.build_submit_info(std::slice::from_ref(cmd))
            }
        };

        // present subgraph 同样按模式委派给对应渲染子系统；GUI 与 debug viewer 只读取该分支导出的
        // render target，避免 realtime/offline 两套资源在同一帧互相暴露状态。
        let present_submit = match self.render_mode {
            RenderMode::Realtime => {
                let mut graph = RenderGraphBuilder::new();
                graph.signal_semaphore(RgSemaphoreInfo::timeline(
                    ctx.timeline.handle(),
                    ash::vk::PipelineStageFlags2::BOTTOM_OF_PIPE,
                    frame_id,
                ));
                let present_targets = self.realtime.contribute_present_passes(
                    &mut graph,
                    &subsystem_ctx,
                    &self.path_tracing_common_settings,
                    selected_debug_image_id,
                );
                self.selection_outline.contribute_passes(
                    &mut graph,
                    ctx,
                    present_targets.present_image,
                    ctx.present.swapchain_image_info().image_extent,
                    self.selected_submesh,
                );
                self.coordinate_gizmo.contribute_passes(
                    &mut graph,
                    ctx,
                    present_targets.present_image,
                    ctx.present.swapchain_image_info().image_extent,
                );
                self.imgui.contribute_passes(
                    &mut graph,
                    &subsystem_ctx,
                    present_targets.present_image,
                    ctx.present.swapchain_image_info().image_extent,
                );

                let compiled_graph = graph.compile();
                if log::log_enabled!(log::Level::Debug) {
                    static PRINT_RT_PRESENT_DEBUG_INFO: std::sync::Once = std::sync::Once::new();
                    PRINT_RT_PRESENT_DEBUG_INFO.call_once(|| {
                        compiled_graph.print_execution_plan();
                    });
                }

                let cmd = self.realtime.present_cmd(frame_label);
                cmd.begin(ash::vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT, "rt-present-graph");
                compiled_graph.execute(cmd, ctx.record_ctx.gfx_resource_registry);
                cmd.end();
                compiled_graph.build_submit_info(std::slice::from_ref(cmd))
            }
            RenderMode::Offline => {
                let mut graph = RenderGraphBuilder::new();
                graph.signal_semaphore(RgSemaphoreInfo::timeline(
                    ctx.timeline.handle(),
                    ash::vk::PipelineStageFlags2::BOTTOM_OF_PIPE,
                    frame_id,
                ));
                let present_targets = self.offline.contribute_present_passes(
                    &mut graph,
                    &subsystem_ctx,
                    &self.path_tracing_common_settings,
                    selected_debug_image_id,
                );
                self.selection_outline.contribute_passes(
                    &mut graph,
                    ctx,
                    present_targets.present_image,
                    ctx.present.swapchain_image_info().image_extent,
                    self.selected_submesh,
                );
                self.coordinate_gizmo.contribute_passes(
                    &mut graph,
                    ctx,
                    present_targets.present_image,
                    ctx.present.swapchain_image_info().image_extent,
                );
                self.imgui.contribute_passes(
                    &mut graph,
                    &subsystem_ctx,
                    present_targets.present_image,
                    ctx.present.swapchain_image_info().image_extent,
                );

                let compiled_graph = graph.compile();
                if log::log_enabled!(log::Level::Debug) {
                    static PRINT_OFFLINE_PRESENT_DEBUG_INFO: std::sync::Once = std::sync::Once::new();
                    PRINT_OFFLINE_PRESENT_DEBUG_INFO.call_once(|| {
                        compiled_graph.print_execution_plan();
                    });
                }

                let cmd = self.offline.present_cmd(frame_label);
                cmd.begin(ash::vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT, "offline-present-graph");
                compiled_graph.execute(cmd, ctx.record_ctx.gfx_resource_registry);
                cmd.end();
                compiled_graph.build_submit_info(std::slice::from_ref(cmd))
            }
        };

        // 两种模式都保持 compute -> present 的提交顺序。timeline signal 放在 present graph，
        // 因此上层 runtime 只需要等待同一个 frame_id 即可观察最终 swapchain 写入完成。
        ctx.queue_ctx.gfx_queue().submit(vec![compute_submit, present_submit], None);
    }

    fn render_view(&self) -> RenderView {
        self.camera_controller.camera().render_view()
    }
}
