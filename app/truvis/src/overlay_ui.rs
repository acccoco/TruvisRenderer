//! Truvis App 级 ImGui overlay 编排。
//!
//! 本模块只决定“哪些 section 以什么布局绘制”。具体控件仍复用 app-kit 的
//! `DebugInfoOverlay` / `PipelineControlsOverlay`，Debug Images 的选择状态与
//! texture 映射仍归 `GuiPlugin` 持有。这里不接触 RenderGraph、GPU resource
//! 生命周期或 GUI draw data 上传，调用方必须在 `GuiPlugin::begin_frame` 与
//! `GuiPlugin::end_frame` 之间调用 `TruvisOverlayUi::build`。

use app_kit::gui_plugin::GuiPlugin;
use app_kit::overlay::{DebugInfoOverlay, FrameStatsOverlayData, PipelineControlsOverlay};
use app_kit::render_pipeline::RenderMode;
use app_kit::render_pipeline::common_settings::PathTracingCommonSettings;
use app_kit::render_pipeline::offline_render_graph::OfflinePipelineSettings;
use app_kit::render_pipeline::rt_render_graph::RtPipelineSettings;
use truvis_render_runtime::ray_cast::RayCastResult;
use truvis_render_runtime::state::dlss_options::DlssOptions;
use truvis_world::World;
use truvis_world::components::material::MaterialData;

use crate::truvis_app::ClickRayCastProbe;

const DEFAULT_WINDOW_MARGIN: f32 = 10.0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OverlayLayoutMode {
    SeparateWindows,
    VerticalStack,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OverlayTag {
    Diagnostics,
    Rendering,
    Upscaling,
    Picking,
    Images,
}

impl OverlayTag {
    const STACK_ORDER: [Self; 5] = [
        Self::Diagnostics,
        Self::Rendering,
        Self::Upscaling,
        Self::Picking,
        Self::Images,
    ];
}

#[derive(Clone, Copy)]
pub struct OverlayWindowOptions {
    pub title: &'static str,
    pub position: [f32; 2],
    pub size: [f32; 2],
    pub condition: imgui::Condition,
    pub flags: imgui::WindowFlags,
    pub visible: bool,
}

#[derive(Clone, Copy)]
pub struct OverlayWindowSet {
    pub diagnostics: OverlayWindowOptions,
    pub rendering: OverlayWindowOptions,
    pub upscaling: OverlayWindowOptions,
    pub picking: OverlayWindowOptions,
    pub images: OverlayWindowOptions,
    pub stack: OverlayWindowOptions,
}

impl OverlayWindowSet {
    fn window(self, tag: OverlayTag) -> OverlayWindowOptions {
        match tag {
            OverlayTag::Diagnostics => self.diagnostics,
            OverlayTag::Rendering => self.rendering,
            OverlayTag::Upscaling => self.upscaling,
            OverlayTag::Picking => self.picking,
            OverlayTag::Images => self.images,
        }
    }
}

impl Default for OverlayWindowSet {
    fn default() -> Self {
        Self {
            diagnostics: OverlayWindowOptions {
                title: "Diagnostics",
                position: [10.0, 10.0],
                size: [340.0, 170.0],
                condition: imgui::Condition::FirstUseEver,
                flags: imgui::WindowFlags::empty(),
                visible: true,
            },
            rendering: OverlayWindowOptions {
                title: "Controls",
                position: [10.0, 200.0],
                size: [340.0, 360.0],
                condition: imgui::Condition::FirstUseEver,
                flags: imgui::WindowFlags::empty(),
                visible: true,
            },
            upscaling: OverlayWindowOptions {
                title: "Upscaling",
                position: [360.0, 200.0],
                size: [300.0, 120.0],
                condition: imgui::Condition::FirstUseEver,
                flags: imgui::WindowFlags::empty(),
                visible: false,
            },
            picking: OverlayWindowOptions {
                title: "Raycast",
                position: [10.0, 420.0],
                size: [430.0, 430.0],
                condition: imgui::Condition::FirstUseEver,
                flags: imgui::WindowFlags::empty(),
                visible: true,
            },
            images: OverlayWindowOptions {
                title: "Debug Images",
                position: [370.0, 10.0],
                size: [420.0, 360.0],
                condition: imgui::Condition::FirstUseEver,
                flags: imgui::WindowFlags::empty(),
                visible: true,
            },
            stack: OverlayWindowOptions {
                title: "Truvis Overlay",
                position: [10.0, 200.0],
                size: [430.0, 650.0],
                condition: imgui::Condition::FirstUseEver,
                flags: imgui::WindowFlags::empty(),
                visible: true,
            },
        }
    }
}

#[derive(Clone, Copy)]
pub struct OverlaySectionVisibility {
    pub diagnostics: bool,
    pub rendering: bool,
    pub upscaling: bool,
    pub picking: bool,
    pub images: bool,
}

impl OverlaySectionVisibility {
    fn is_visible(self, tag: OverlayTag) -> bool {
        match tag {
            OverlayTag::Diagnostics => self.diagnostics,
            OverlayTag::Rendering => self.rendering,
            OverlayTag::Upscaling => self.upscaling,
            OverlayTag::Picking => self.picking,
            OverlayTag::Images => self.images,
        }
    }
}

impl Default for OverlaySectionVisibility {
    fn default() -> Self {
        Self {
            diagnostics: true,
            rendering: true,
            upscaling: true,
            picking: true,
            images: true,
        }
    }
}

pub struct TruvisOverlayOptions {
    pub layout: OverlayLayoutMode,
    pub windows: OverlayWindowSet,
    pub sections: OverlaySectionVisibility,
}

impl Default for TruvisOverlayOptions {
    fn default() -> Self {
        Self {
            layout: OverlayLayoutMode::SeparateWindows,
            windows: OverlayWindowSet::default(),
            sections: OverlaySectionVisibility::default(),
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

    pub(crate) fn build(&mut self, frame: TruvisOverlayFrame<'_>) {
        match self.options.layout {
            OverlayLayoutMode::SeparateWindows => self.build_separate_windows(frame),
            OverlayLayoutMode::VerticalStack => self.build_vertical_stack(frame),
        }
    }

    fn build_separate_windows(&self, frame: TruvisOverlayFrame<'_>) {
        let TruvisOverlayFrame {
            ui,
            stats,
            mut pipeline,
            raycast,
            debug_images,
        } = frame;

        if self.section_visible(OverlayTag::Diagnostics) && self.window(OverlayTag::Diagnostics).visible {
            DebugInfoOverlay::build_frame_stats_hud(ui, &stats);
        }

        let upscaling_is_separate =
            self.section_visible(OverlayTag::Upscaling) && self.window(OverlayTag::Upscaling).visible;
        if self.options.windows.stack.visible
            && (self.section_visible(OverlayTag::Rendering) || self.section_visible(OverlayTag::Picking))
        {
            Self::build_window_with_options(ui, self.options.windows.stack, || {
                if self.section_visible(OverlayTag::Rendering) {
                    Self::draw_stack_section_header(ui, OverlayTag::Rendering);
                    // 默认 separate-style 布局把渲染选项与点选结果放进同一个 App 级主面板；
                    // 控件本身仍复用 app-kit section，避免把 pipeline owner 状态迁入布局层。
                    Self::draw_controls_contents(
                        ui,
                        &mut pipeline,
                        self.section_visible(OverlayTag::Upscaling) && !upscaling_is_separate,
                    );
                }

                if self.section_visible(OverlayTag::Picking) {
                    Self::draw_stack_section_header(ui, OverlayTag::Picking);
                    Self::draw_raycast_contents(ui, &raycast);
                }
            });
        }
        if upscaling_is_separate {
            self.build_window(ui, OverlayTag::Upscaling, || {
                PipelineControlsOverlay::build_dlss_section_for_mode(ui, *pipeline.render_mode, pipeline.dlss_options);
            });
        }
        if self.section_visible(OverlayTag::Images) {
            self.build_right_aligned_image_window(ui, &stats, || {
                debug_images.gui.build_debug_image_viewer_contents(ui);
            });
        }
    }

    fn build_vertical_stack(&self, frame: TruvisOverlayFrame<'_>) {
        let TruvisOverlayFrame {
            ui,
            stats,
            mut pipeline,
            raycast,
            debug_images,
        } = frame;
        let stack = self.options.windows.stack;
        if !stack.visible {
            return;
        }

        Self::build_window_with_options(ui, stack, || {
            for tag in OverlayTag::STACK_ORDER {
                if !self.section_visible(tag) {
                    continue;
                }
                Self::draw_stack_section_header(ui, tag);
                match tag {
                    OverlayTag::Diagnostics => DebugInfoOverlay::build_frame_stats_section(ui, &stats),
                    OverlayTag::Rendering => Self::draw_rendering_sections(ui, &mut pipeline),
                    OverlayTag::Upscaling => PipelineControlsOverlay::build_dlss_section_for_mode(
                        ui,
                        *pipeline.render_mode,
                        pipeline.dlss_options,
                    ),
                    OverlayTag::Picking => Self::draw_raycast_contents(ui, &raycast),
                    OverlayTag::Images => debug_images.gui.build_debug_image_viewer_contents(ui),
                }
            }
        });
    }

    fn section_visible(&self, tag: OverlayTag) -> bool {
        self.options.sections.is_visible(tag)
    }

    fn window(&self, tag: OverlayTag) -> OverlayWindowOptions {
        self.options.windows.window(tag)
    }

    fn build_window(&self, ui: &imgui::Ui, tag: OverlayTag, build: impl FnOnce()) {
        let options = self.window(tag);
        if !options.visible {
            return;
        }
        Self::build_window_with_options(ui, options, build);
    }

    fn build_right_aligned_image_window(
        &self,
        ui: &imgui::Ui,
        stats: &FrameStatsOverlayData<'_>,
        build: impl FnOnce(),
    ) {
        let options = self.window(OverlayTag::Images);
        if !options.visible {
            return;
        }

        let right_aligned_x =
            (stats.swapchain_extent.width as f32 - options.size[0] - DEFAULT_WINDOW_MARGIN).max(DEFAULT_WINDOW_MARGIN);
        ui.window(options.title)
            .position([right_aligned_x, options.position[1]], imgui::Condition::Always)
            .size(options.size, options.condition)
            .flags(options.flags)
            .build(build);
    }

    fn build_window_with_options(ui: &imgui::Ui, options: OverlayWindowOptions, build: impl FnOnce()) {
        ui.window(options.title)
            .position(options.position, options.condition)
            .size(options.size, options.condition)
            .flags(options.flags)
            .build(build);
    }

    fn draw_stack_section_header(ui: &imgui::Ui, tag: OverlayTag) {
        ui.separator();
        ui.text(match tag {
            OverlayTag::Diagnostics => "Diagnostics",
            OverlayTag::Rendering => "Rendering",
            OverlayTag::Upscaling => "Upscaling",
            OverlayTag::Picking => "Picking",
            OverlayTag::Images => "Images",
        });
        ui.separator();
    }

    fn draw_controls_contents(ui: &imgui::Ui, pipeline: &mut PipelineControlsData<'_>, include_dlss: bool) {
        Self::normalize_render_mode(pipeline);
        PipelineControlsOverlay::build_render_mode_section(
            ui,
            pipeline.render_mode,
            pipeline.offline_mode_supported(),
            pipeline.offline_sample_count,
        );

        if include_dlss {
            ui.separator();
            PipelineControlsOverlay::build_dlss_section_for_mode(ui, *pipeline.render_mode, pipeline.dlss_options);
        }

        ui.separator();
        PipelineControlsOverlay::build_mode_specific_sections(
            ui,
            *pipeline.render_mode,
            pipeline.common_settings.as_deref_mut(),
            pipeline.rt_settings.as_deref_mut(),
            pipeline.offline_settings.as_deref_mut(),
        );
    }

    fn draw_rendering_sections(ui: &imgui::Ui, pipeline: &mut PipelineControlsData<'_>) {
        Self::normalize_render_mode(pipeline);
        PipelineControlsOverlay::build_render_mode_section(
            ui,
            pipeline.render_mode,
            pipeline.offline_mode_supported(),
            pipeline.offline_sample_count,
        );
        ui.separator();
        PipelineControlsOverlay::build_mode_specific_sections(
            ui,
            *pipeline.render_mode,
            pipeline.common_settings.as_deref_mut(),
            pipeline.rt_settings.as_deref_mut(),
            pipeline.offline_settings.as_deref_mut(),
        );
    }

    fn normalize_render_mode(pipeline: &mut PipelineControlsData<'_>) {
        if !pipeline.offline_mode_supported() {
            *pipeline.render_mode = RenderMode::Realtime;
        }
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
            ui.text(format!("Error: {error}"));
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
                Self::draw_material_info(ui, raycast.world.material_data(hit.material));
                ui.text(format!("Hit T: {:.3}", hit.hit_t));
                ui.text(format!(
                    "Position: ({:.2}, {:.2}, {:.2})",
                    hit.position_ws.x, hit.position_ws.y, hit.position_ws.z
                ));
                ui.text(format!("Normal: ({:.2}, {:.2}, {:.2})", hit.normal_ws.x, hit.normal_ws.y, hit.normal_ws.z));
                ui.text(format!("UV: ({:.3}, {:.3})", hit.uv.x, hit.uv.y));
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

        ui.text(format!("Material name: {}", material.name));
        ui.text(format!(
            "Base color: ({:.3}, {:.3}, {:.3}, {:.3})",
            material.base_color.x, material.base_color.y, material.base_color.z, material.base_color.w
        ));
        ui.text(format!(
            "Emissive: ({:.3}, {:.3}, {:.3}, {:.3})",
            material.emissive.x, material.emissive.y, material.emissive.z, material.emissive.w
        ));
        ui.text(format!("Metallic: {:.3}", material.metallic));
        ui.text(format!("Roughness: {:.3}", material.roughness));
        ui.text(format!("Opaque: {:.3}", material.opaque));
        ui.text(format!("Diffuse texture: {:?}", material.diffuse_texture));
        ui.text(format!("Normal texture: {:?}", material.normal_texture));
    }
}

pub(crate) struct TruvisOverlayFrame<'a> {
    pub(crate) ui: &'a imgui::Ui,
    pub(crate) stats: FrameStatsOverlayData<'a>,
    pub(crate) pipeline: PipelineControlsData<'a>,
    pub(crate) raycast: RaycastOverlayData<'a>,
    pub(crate) debug_images: DebugImageViewerData<'a>,
}

pub(crate) struct PipelineControlsData<'a> {
    pub(crate) render_mode: &'a mut RenderMode,
    pub(crate) dlss_options: &'a mut DlssOptions,
    pub(crate) common_settings: Option<&'a mut PathTracingCommonSettings>,
    pub(crate) rt_settings: Option<&'a mut RtPipelineSettings>,
    pub(crate) offline_settings: Option<&'a mut OfflinePipelineSettings>,
    pub(crate) offline_sample_count: Option<u32>,
}

impl PipelineControlsData<'_> {
    fn offline_mode_supported(&self) -> bool {
        self.offline_settings.is_some()
    }
}

pub(crate) struct RaycastOverlayData<'a> {
    pub(crate) probe: &'a ClickRayCastProbe,
    pub(crate) world: &'a World,
}

pub(crate) struct DebugImageViewerData<'a> {
    pub(crate) gui: &'a GuiPlugin,
}
