//! Truvis Renderer 级 ImGui overlay 编排。
//!
//! 本模块只决定“哪些 section 以什么布局绘制”。具体控件复用独立的诊断和渲染设置 crate，
//! `DebugInfoOverlay` / `RenderControlsOverlay`，Debug Images 只在这里修改 Renderer-owned
//! `DebugImageSelection`。这里不接触 RenderGraph、GPU resource 生命周期或 GUI draw
//! data 上传；调用方在 `ImGuiSubsystem::build_frame` 的闭包内调用 `TruvisOverlayUi::build`。

use renderer_imgui::{DebugImageSelectorView, DebugInfoOverlay, FrameStatsOverlayData};
use renderer_kit::debug_image::{DebugImageOption, DebugImageSelection};
use renderer_render_ui::RenderControlsOverlay;
use renderer_rendering::{OfflineRenderSettings, PathTracingCommonSettings, RealtimeRenderSettings, RenderMode};
use truvis_render_runtime::ray_cast::RayCastResult;
use truvis_render_runtime::state::dlss_options::DlssOptions;
use truvis_world::GameWorld;
use truvis_world::components::material::{CoverageMode, MaterialClass, MaterialData};

use crate::truvis_renderer::ClickRayCastProbe;

/// 主面板与轻量 HUD 的显示策略；页签和滚动状态由 ImGui context 持有。
pub struct TruvisOverlayOptions {
    pub show_window: bool,
    pub show_fps_hud: bool,
}

impl Default for TruvisOverlayOptions {
    fn default() -> Self {
        Self {
            show_window: true,
            show_fps_hud: true,
        }
    }
}

#[derive(Default)]
pub(crate) struct TruvisOverlayUi {
    options: TruvisOverlayOptions,
}

impl TruvisOverlayUi {
    pub(crate) fn options(&self) -> &TruvisOverlayOptions {
        &self.options
    }

    pub(crate) fn options_mut(&mut self) -> &mut TruvisOverlayOptions {
        &mut self.options
    }

    /// 只组织当前可见控件；配置归一化和 picking 执行由 Renderer 的固定阶段维护。
    pub(crate) fn build(&mut self, frame: TruvisOverlayFrame<'_>) {
        let TruvisOverlayFrame {
            ui,
            stats,
            mut render_controls,
            raycast,
            mut debug_images,
        } = frame;
        if self.options.show_fps_hud {
            DebugInfoOverlay::build_fps_hud(ui, &stats);
        }
        if !self.options.show_window {
            return;
        }

        ui.window("Truvis Overlay")
            .position([10.0, 40.0], imgui::Condition::FirstUseEver)
            .size([320.0, 340.0], imgui::Condition::FirstUseEver)
            .size_constraints([320.0, 240.0], [f32::MAX, f32::MAX])
            .scroll_bar(false)
            .scrollable(false)
            .build(|| {
                RenderControlsOverlay::build_render_mode_section(
                    ui,
                    render_controls.render_mode,
                    render_controls.offline_sample_count,
                );
                if let Some(_tabs) = ui.tab_bar_with_flags("TruvisTabs", imgui::TabBarFlags::FITTING_POLICY_SCROLL) {
                    Self::build_tab(ui, "Render", || Self::draw_render_tab(ui, &mut render_controls));
                    Self::build_tab(ui, "Sky", || Self::draw_sky_tab(ui, &mut render_controls));
                    Self::build_tab(ui, "Post", || Self::draw_post_tab(ui, &mut render_controls));
                    Self::build_tab(ui, "Picking", || Self::draw_raycast_contents(ui, &raycast));
                    Self::build_tab(ui, "Debug", || {
                        Self::draw_debug_tab(ui, &stats, &mut render_controls, &mut debug_images);
                    });
                }
            });
    }

    /// tab 自动压入自己的 ID，使各页 child 的滚动和折叠状态独立且跨切页保留。
    fn build_tab(ui: &imgui::Ui, label: &str, build: impl FnOnce()) {
        if let Some(_tab) = ui.tab_item(label) {
            ui.child_window("Contents").size([0.0, 0.0]).build(|| {
                let _width = ui.push_item_width(ui.content_region_avail()[0] * 0.5);
                build();
            });
        }
    }

    fn draw_render_tab(ui: &imgui::Ui, controls: &mut RenderControlsData<'_>) {
        ui.text("Sampling");
        RenderControlsOverlay::build_sampling_section(
            ui,
            *controls.render_mode,
            controls.common_settings,
            controls.offline_settings,
        );
        ui.separator();
        ui.text("Realtime");
        RenderControlsOverlay::build_realtime_settings_section(ui, *controls.render_mode, controls.realtime_settings);
        ui.separator();
        ui.text("Reconstruction");
        RenderControlsOverlay::build_dlss_section_for_mode(ui, *controls.render_mode, controls.dlss_options);
    }

    fn draw_sky_tab(ui: &imgui::Ui, controls: &mut RenderControlsData<'_>) {
        ui.checkbox("Environment Enabled", controls.sky_enabled);
        RenderControlsOverlay::build_sky_section(
            ui,
            &mut controls.common_settings.sky_sampling_mode,
            controls.sky_brightness,
        );
    }

    fn draw_post_tab(ui: &imgui::Ui, controls: &mut RenderControlsData<'_>) {
        RenderControlsOverlay::build_post_process_section(ui, &mut controls.common_settings.post_process);
    }

    fn draw_debug_tab(
        ui: &imgui::Ui,
        stats: &FrameStatsOverlayData<'_>,
        controls: &mut RenderControlsData<'_>,
        debug_images: &mut DebugImageSelectionData<'_>,
    ) {
        RenderControlsOverlay::build_debug_channel_section(
            ui,
            *controls.render_mode,
            controls.realtime_settings,
            controls.offline_settings,
        );
        ui.separator();
        ui.text("Debug Images");
        debug_images.build_contents(ui, *controls.render_mode);
        ui.separator();
        ui.text("Diagnostics");
        DebugInfoOverlay::build_frame_stats_section(ui, stats);
    }

    fn draw_raycast_contents(ui: &imgui::Ui, raycast: &RaycastOverlayData<'_>) {
        ui.text("Trigger: left mouse click");
        if raycast.probe.has_pending_cast() {
            ui.text("Status: pending");
        } else {
            ui.text("Status: idle");
        }

        if let Some(screen_pos) = raycast.probe.last_screen_pos() {
            ui.text(format!("Last click: ({:.0}, {:.0})", screen_pos.x, screen_pos.y));
        } else {
            ui.text("Last click: never");
        }

        if let Some(last_cast_time_s) = raycast.probe.last_cast_time_s() {
            ui.text(format!("Last cast at: {:.2}s", last_cast_time_s));
        } else {
            ui.text("Last cast: never");
        }
        ui.separator();

        if let Some(error) = raycast.probe.last_error() {
            ui.text_wrapped(format!("Error: {error}"));
            return;
        }

        match raycast.probe.last_result() {
            Some(RayCastResult::Miss) => {
                ui.text("Result: Miss");
            }
            Some(RayCastResult::Hit(hit)) => {
                ui.text("Result: Hit");
                ui.text(format!("Instance: {:?}", hit.instance));
                ui.text(format!("Mesh: {:?}", hit.mesh));
                ui.text(format!("Material: {:?}", hit.material));
                ui.text(format!("Submesh: {}", hit.submesh_index));
                ui.text(format!("Primitive: {}", hit.primitive_index));
                if ui.collapsing_header("Geometry", imgui::TreeNodeFlags::DEFAULT_OPEN) {
                    ui.text(format!("Hit T: {:.3}", hit.hit_t));
                    ui.text(format!(
                        "Position: ({:.2}, {:.2}, {:.2})",
                        hit.position_ws.x, hit.position_ws.y, hit.position_ws.z
                    ));
                    ui.text(format!(
                        "Normal: ({:.2}, {:.2}, {:.2})",
                        hit.normal_ws.x, hit.normal_ws.y, hit.normal_ws.z
                    ));
                    ui.text(format!("UV: ({:.3}, {:.3})", hit.uv.x, hit.uv.y));
                }
                if ui.collapsing_header("Material", imgui::TreeNodeFlags::empty()) {
                    Self::draw_material_info(ui, raycast.world.material_data(hit.material));
                }
            }
            None => {
                ui.text("Result: waiting");
            }
        }
    }

    fn draw_material_info(ui: &imgui::Ui, material: Option<&MaterialData>) {
        let Some(material) = material else {
            ui.text("Material data: unavailable");
            return;
        };

        ui.text_wrapped(format!("Material name: {}", material.name));
        ui.text(format!(
            "Base color: ({:.3}, {:.3}, {:.3}, {:.3})",
            material.base_color.x, material.base_color.y, material.base_color.z, material.base_color.w
        ));
        ui.text(format!("Metallic: {:.3}", material.metallic));
        ui.text(format!("Roughness: {:.3}", material.roughness));
        ui.text(format!("Class: {}", Self::material_class_label(material.class)));
        ui.text(format!("Coverage: {}", Self::coverage_label(material.coverage)));
        ui.text(format!("Alpha factor: {:.3}", material.base_color.w));
        ui.text(format!("Diffuse texture: {:?}", material.textures[0]));
        ui.text(format!("Normal texture: {:?}", material.textures[2]));
    }

    fn material_class_label(class: MaterialClass) -> String {
        match class {
            MaterialClass::Surface => "Surface".to_string(),
            MaterialClass::Transmission { opacity, ior } => format!("Transmission opacity={opacity:.3} ior={ior:.3}"),
            MaterialClass::Emissive { radiance } => {
                format!("Emissive radiance=({:.3}, {:.3}, {:.3})", radiance.x, radiance.y, radiance.z)
            }
        }
    }

    fn coverage_label(coverage: CoverageMode) -> String {
        match coverage {
            CoverageMode::Opaque => "Opaque".to_string(),
            CoverageMode::AlphaMask { alpha_cutoff } => format!("AlphaMask cutoff={alpha_cutoff:.3}"),
        }
    }
}

pub(crate) struct TruvisOverlayFrame<'a> {
    pub(crate) ui: &'a imgui::Ui,
    pub(crate) stats: FrameStatsOverlayData<'a>,
    pub(crate) render_controls: RenderControlsData<'a>,
    pub(crate) raycast: RaycastOverlayData<'a>,
    pub(crate) debug_images: DebugImageSelectionData<'a>,
}

pub(crate) struct RenderControlsData<'a> {
    pub(crate) render_mode: &'a mut RenderMode,
    pub(crate) dlss_options: &'a mut DlssOptions,
    pub(crate) common_settings: &'a mut PathTracingCommonSettings,
    pub(crate) realtime_settings: &'a mut RealtimeRenderSettings,
    pub(crate) offline_settings: &'a mut OfflineRenderSettings,
    pub(crate) offline_sample_count: u32,
    pub(crate) sky_brightness: &'a mut f32,
    pub(crate) sky_enabled: &'a mut bool,
}

pub(crate) struct RaycastOverlayData<'a> {
    pub(crate) probe: &'a ClickRayCastProbe,
    pub(crate) world: &'a GameWorld,
}

pub(crate) struct DebugImageSelectionData<'a> {
    pub(crate) selection: &'a mut DebugImageSelection,
    pub(crate) realtime_options: &'static [DebugImageOption],
    pub(crate) offline_options: &'static [DebugImageOption],
}

impl DebugImageSelectionData<'_> {
    fn build_contents(&mut self, ui: &imgui::Ui, render_mode: RenderMode) {
        let options = match render_mode {
            RenderMode::Realtime => self.realtime_options,
            RenderMode::Offline => self.offline_options,
        };
        DebugImageSelectorView::build_contents(ui, self.selection, options);
    }
}
