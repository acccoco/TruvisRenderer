use ash::vk;
use truvis_app_frame::plugin_api::Plugin;
use truvis_render_runtime::state::dlss_options::DlssOptions;
use truvis_render_runtime::state::dlss_sr::DlssSrMode;

use crate::camera::Camera;
use crate::render_pipeline::rt_render_graph::{RtDebugChannel, RtPipelineSettings, RtSkySamplingMode};

#[derive(Default)]
pub struct DebugInfoOverlay;

impl Plugin for DebugInfoOverlay {}

impl DebugInfoOverlay {
    pub fn build_overlay_ui(
        &mut self,
        ui: &imgui::Ui,
        camera: &Camera,
        swapchain_extent: vk::Extent2D,
        accum_frames_num: usize,
        delta_time_s: f32,
    ) {
        ui.window("##overlay")
            .position([0.0, 0.0], imgui::Condition::Always)
            .size([swapchain_extent.width as f32, swapchain_extent.height as f32], imgui::Condition::Always)
            .flags(
                imgui::WindowFlags::NO_TITLE_BAR
                    | imgui::WindowFlags::NO_RESIZE
                    | imgui::WindowFlags::NO_MOVE
                    | imgui::WindowFlags::NO_SCROLLBAR
                    | imgui::WindowFlags::NO_SCROLL_WITH_MOUSE
                    | imgui::WindowFlags::NO_COLLAPSE
                    | imgui::WindowFlags::NO_BACKGROUND
                    | imgui::WindowFlags::NO_SAVED_SETTINGS
                    | imgui::WindowFlags::NO_MOUSE_INPUTS
                    | imgui::WindowFlags::NO_FOCUS_ON_APPEARING
                    | imgui::WindowFlags::NO_BRING_TO_FRONT_ON_FOCUS
                    | imgui::WindowFlags::NO_NAV_INPUTS
                    | imgui::WindowFlags::NO_NAV_FOCUS,
            )
            .build(|| {
                ui.set_cursor_pos([5.0, 5.0]);
                ui.text(format!("FPS: {:.2}", 1.0 / delta_time_s));
                ui.text(format!("swapchain: {:.0}x{:.0}", swapchain_extent.width, swapchain_extent.height));
                ui.text(format!(
                    "CameraPos: ({:.2}, {:.2}, {:.2})",
                    camera.position.x, camera.position.y, camera.position.z
                ));
                ui.text(format!(
                    "CameraEuler: ({:.2}, {:.2}, {:.2})",
                    camera.euler_yaw_deg, camera.euler_pitch_deg, camera.euler_roll_deg
                ));
                ui.text(format!(
                    "CameraForward: ({:.2}, {:.2}, {:.2})",
                    camera.camera_forward().x,
                    camera.camera_forward().y,
                    camera.camera_forward().z
                ));
                ui.text(format!("CameraAspect: {:.2}", camera.asp));
                ui.text(format!("CameraFov(Vertical): {:.2}\u{00b0}", camera.fov_deg_vertical));
                ui.text(format!("Accum Frames: {}", accum_frames_num));
                ui.new_line();
            });
    }
}

#[derive(Default)]
pub struct PipelineControlsOverlay;

impl Plugin for PipelineControlsOverlay {}

impl PipelineControlsOverlay {
    pub fn build_overlay_ui(
        &mut self,
        ui: &imgui::Ui,
        dlss_options: &mut DlssOptions,
        rt_settings: Option<&mut RtPipelineSettings>,
    ) {
        ui.window("Controls")
            .position([10.0, 200.0], imgui::Condition::FirstUseEver)
            .size([320.0, 300.0], imgui::Condition::FirstUseEver)
            .build(|| {
                // RR 作为独立开关接入，不放进 SR/DLAA 质量挡位下拉框。
                if let Some(_combo) = ui.begin_combo("DLSS SR", dlss_options.dlss_sr_mode.label()) {
                    for mode in DlssSrMode::ALL {
                        if ui.selectable_config(mode.label()).selected(dlss_options.dlss_sr_mode == mode).build() {
                            dlss_options.dlss_sr_mode = mode;
                        }
                    }
                }
                ui.checkbox("DLSS RR", &mut dlss_options.dlss_rr_enabled);

                if let Some(rt_settings) = rt_settings {
                    ui.separator();
                    if let Some(_combo) = ui.begin_combo("RT debug", rt_settings.debug_channel.label()) {
                        for channel in RtDebugChannel::ALL {
                            if ui
                                .selectable_config(channel.label())
                                .selected(rt_settings.debug_channel == channel)
                                .build()
                            {
                                rt_settings.debug_channel = channel;
                            }
                        }
                    }
                    if let Some(_combo) = ui.begin_combo("Sky sampling", rt_settings.sky_sampling_mode.label()) {
                        for mode in RtSkySamplingMode::ALL {
                            if ui
                                .selectable_config(mode.label())
                                .selected(rt_settings.sky_sampling_mode == mode)
                                .build()
                            {
                                rt_settings.sky_sampling_mode = mode;
                            }
                        }
                    }

                    ui.separator();
                    ui.text("Tone Mapping");
                    ui.slider_config("Exposure EV", -8.0_f32, 8.0_f32)
                        .display_format("%.2f")
                        .build(&mut rt_settings.tone_mapping.exposure_ev);
                    ui.slider_config("ACES Strength", 0.0_f32, 1.0_f32)
                        .display_format("%.2f")
                        .build(&mut rt_settings.tone_mapping.aces_strength);
                    ui.slider_config("White Point", 1.0_f32, 32.0_f32)
                        .display_format("%.2f")
                        .build(&mut rt_settings.tone_mapping.aces_white_point);
                }
            });
    }
}
