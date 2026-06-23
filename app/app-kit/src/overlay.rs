use ash::vk;
use truvis_app_frame::plugin_api::Plugin;
use truvis_render_runtime::state::dlss_options::DlssOptions;
use truvis_render_runtime::state::dlss_sr::DlssSrMode;

use crate::camera::Camera;
use crate::render_pipeline::RenderMode;
use crate::render_pipeline::common_settings::{PathTracingCommonSettings, RtSkySamplingMode};
use crate::render_pipeline::offline_render_graph::OfflinePipelineSettings;
use crate::render_pipeline::rt_render_graph::{RtDebugChannel, RtPipelineSettings, RtRestirDiMode, RtSharcMode};

pub struct FrameStatsOverlayData<'a> {
    pub camera: &'a Camera,
    pub swapchain_extent: vk::Extent2D,
    pub accum_frames_num: usize,
    pub delta_time_s: f32,
}

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
        let stats = FrameStatsOverlayData {
            camera,
            swapchain_extent,
            accum_frames_num,
            delta_time_s,
        };
        Self::build_frame_stats_hud(ui, &stats);
    }

    pub fn build_frame_stats_hud(ui: &imgui::Ui, stats: &FrameStatsOverlayData<'_>) {
        ui.window("##overlay")
            .position([0.0, 0.0], imgui::Condition::Always)
            .size(
                [
                    stats.swapchain_extent.width as f32,
                    stats.swapchain_extent.height as f32,
                ],
                imgui::Condition::Always,
            )
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
                Self::build_frame_stats_section(ui, stats);
            });
    }

    pub fn build_frame_stats_section(ui: &imgui::Ui, stats: &FrameStatsOverlayData<'_>) {
        let camera = stats.camera;
        ui.text(format!("FPS: {:.2}", 1.0 / stats.delta_time_s));
        ui.text(format!("swapchain: {:.0}x{:.0}", stats.swapchain_extent.width, stats.swapchain_extent.height));
        ui.text(format!("CameraPos: ({:.2}, {:.2}, {:.2})", camera.position.x, camera.position.y, camera.position.z));
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
        ui.text(format!("Accum Frames: {}", stats.accum_frames_num));
        ui.new_line();
    }
}

#[derive(Default)]
pub struct PipelineControlsOverlay;

impl Plugin for PipelineControlsOverlay {}

impl PipelineControlsOverlay {
    pub fn build_overlay_ui(
        &mut self,
        ui: &imgui::Ui,
        render_mode: &mut RenderMode,
        dlss_options: &mut DlssOptions,
        mut common_settings: Option<&mut PathTracingCommonSettings>,
        mut rt_settings: Option<&mut RtPipelineSettings>,
        mut offline_settings: Option<&mut OfflinePipelineSettings>,
        offline_sample_count: Option<u32>,
    ) {
        let offline_mode_supported = offline_settings.is_some();
        // 部分 sample app 只复用 Controls overlay，并不创建 OfflinePipeline。这里把外部传入
        // 的 mode 收敛回 Realtime，避免 UI 暴露一个当前 app 无法实际调度的离线分支。
        if !offline_mode_supported {
            *render_mode = RenderMode::Realtime;
        }

        ui.window("Controls")
            .position([10.0, 200.0], imgui::Condition::FirstUseEver)
            .size([340.0, 360.0], imgui::Condition::FirstUseEver)
            .build(|| {
                Self::build_render_mode_section(ui, render_mode, offline_mode_supported, offline_sample_count);

                ui.separator();
                Self::build_dlss_section_for_mode(ui, *render_mode, dlss_options);

                ui.separator();
                Self::build_mode_specific_sections(
                    ui,
                    *render_mode,
                    common_settings.as_deref_mut(),
                    rt_settings.as_deref_mut(),
                    offline_settings.as_deref_mut(),
                );
            });
    }

    pub fn build_render_mode_section(
        ui: &imgui::Ui,
        render_mode: &mut RenderMode,
        offline_mode_supported: bool,
        offline_sample_count: Option<u32>,
    ) {
        if offline_mode_supported {
            if let Some(_combo) = ui.begin_combo("Render Mode", render_mode.label()) {
                for mode in RenderMode::ALL {
                    if ui.selectable_config(mode.label()).selected(*render_mode == mode).build() {
                        *render_mode = mode;
                    }
                }
            }
        }

        if *render_mode == RenderMode::Offline {
            ui.text(format!("Offline Samples: {}", offline_sample_count.unwrap_or(0)));
        }
    }

    pub fn build_dlss_section_for_mode(ui: &imgui::Ui, render_mode: RenderMode, dlss_options: &mut DlssOptions) {
        // DLSS SR/RR 依赖 realtime RT 管线产出的 GBuffer、motion vector 和历史资源；
        // 离线模式保留控件位置但禁用，明确它们不会影响 reference 累计采样状态。
        let realtime_mode = render_mode == RenderMode::Realtime;
        ui.disabled(!realtime_mode, || {
            Self::build_dlss_section(ui, dlss_options);
        });
        if !realtime_mode {
            ui.text_disabled("DLSS controls are realtime only");
        }
    }

    pub fn build_dlss_section(ui: &imgui::Ui, dlss_options: &mut DlssOptions) {
        // RR 作为独立开关接入，不放进 SR/DLAA 质量挡位下拉框。
        if let Some(_combo) = ui.begin_combo("DLSS SR", dlss_options.dlss_sr_mode.label()) {
            for mode in DlssSrMode::ALL {
                if ui.selectable_config(mode.label()).selected(dlss_options.dlss_sr_mode == mode).build() {
                    dlss_options.dlss_sr_mode = mode;
                }
            }
        }
        ui.checkbox("DLSS RR", &mut dlss_options.dlss_rr_enabled);
    }

    pub fn build_mode_specific_sections(
        ui: &imgui::Ui,
        render_mode: RenderMode,
        common_settings: Option<&mut PathTracingCommonSettings>,
        rt_settings: Option<&mut RtPipelineSettings>,
        offline_settings: Option<&mut OfflinePipelineSettings>,
    ) {
        let mut common_settings = common_settings;
        let mut rt_settings = rt_settings;
        let mut offline_settings = offline_settings;
        match render_mode {
            RenderMode::Realtime => {
                if let Some(rt_settings) = rt_settings.as_deref_mut() {
                    Self::build_realtime_rt_section(ui, rt_settings, common_settings.as_deref_mut(), false);
                }
            }
            RenderMode::Offline => {
                if let Some(offline_settings) = offline_settings.as_deref_mut() {
                    Self::build_offline_rt_section(ui, offline_settings, common_settings.as_deref_mut());
                }
                if let Some(rt_settings) = rt_settings.as_deref_mut() {
                    ui.disabled(true, || {
                        Self::build_restir_section(ui, &mut rt_settings.restir_di_mode);
                    });
                    ui.text_disabled("ReSTIR DI is realtime only");
                }
            }
        }
    }

    pub fn build_realtime_rt_section(
        ui: &imgui::Ui,
        rt_settings: &mut RtPipelineSettings,
        mut common_settings: Option<&mut PathTracingCommonSettings>,
        restir_disabled: bool,
    ) {
        if let Some(_combo) = ui.begin_combo("RT debug", rt_settings.debug_channel.label()) {
            for channel in RtDebugChannel::ALL {
                if ui.selectable_config(channel.label()).selected(rt_settings.debug_channel == channel).build() {
                    rt_settings.debug_channel = channel;
                }
            }
        }
        if let Some(common_settings) = common_settings.as_deref_mut() {
            Self::build_common_sampling_section(
                ui,
                &mut common_settings.sky_sampling_mode,
                &mut common_settings.sky_brightness,
                &mut common_settings.emissive_nee_enabled,
                &mut common_settings.analytic_nee_enabled,
            );
        }
        ui.disabled(restir_disabled, || {
            Self::build_restir_section(ui, &mut rt_settings.restir_di_mode);
        });
        if restir_disabled {
            ui.text_disabled("ReSTIR DI is realtime only");
        }
        // SHARC 也是 realtime-only 能力，沿用同一 disabled 语义。
        ui.disabled(restir_disabled, || {
            Self::build_sharc_section(ui, &mut rt_settings.sharc_mode, &mut rt_settings.sharc_scene_scale);
        });
        if let Some(common_settings) = common_settings.as_deref_mut() {
            Self::build_tone_mapping_section(ui, &mut common_settings.tone_mapping);
        }
    }

    pub fn build_offline_rt_section(
        ui: &imgui::Ui,
        offline_settings: &mut OfflinePipelineSettings,
        mut common_settings: Option<&mut PathTracingCommonSettings>,
    ) {
        // 离线 raygen 不维护 ReSTIR reservoir，也不执行 realtime ReSTIR phase。若离线设置来自旧配置
        // 或未来入口而落在 ReSTIR-only debug channel，这里先收敛到 Final，避免展示无来源图像。
        if !Self::offline_supports_debug_channel(offline_settings.debug_channel) {
            offline_settings.debug_channel = RtDebugChannel::Final;
        }

        if let Some(_combo) = ui.begin_combo("RT debug", offline_settings.debug_channel.label()) {
            for channel in RtDebugChannel::ALL {
                if !Self::offline_supports_debug_channel(channel) {
                    continue;
                }
                if ui.selectable_config(channel.label()).selected(offline_settings.debug_channel == channel).build() {
                    offline_settings.debug_channel = channel;
                }
            }
        }
        let mut ray_dispatch_count = offline_settings.effective_ray_dispatch_count() as i32;
        ui.slider_config(
            "RT Dispatches / Frame",
            OfflinePipelineSettings::MIN_RAY_DISPATCH_COUNT as i32,
            OfflinePipelineSettings::MAX_RAY_DISPATCH_COUNT as i32,
        )
        .display_format("%d")
        .build(&mut ray_dispatch_count);
        offline_settings.set_ray_dispatch_count(ray_dispatch_count as u32);
        if let Some(common_settings) = common_settings.as_deref_mut() {
            Self::build_common_sampling_section(
                ui,
                &mut common_settings.sky_sampling_mode,
                &mut common_settings.sky_brightness,
                &mut common_settings.emissive_nee_enabled,
                &mut common_settings.analytic_nee_enabled,
            );
        }
        if let Some(common_settings) = common_settings.as_deref_mut() {
            Self::build_tone_mapping_section(ui, &mut common_settings.tone_mapping);
        }
    }

    pub fn build_common_sampling_section(
        ui: &imgui::Ui,
        sky_sampling_mode: &mut RtSkySamplingMode,
        sky_brightness: &mut f32,
        emissive_nee_enabled: &mut bool,
        analytic_nee_enabled: &mut bool,
    ) {
        if let Some(_combo) = ui.begin_combo("Sky sampling", sky_sampling_mode.label()) {
            for mode in RtSkySamplingMode::ALL {
                if ui.selectable_config(mode.label()).selected(*sky_sampling_mode == mode).build() {
                    *sky_sampling_mode = mode;
                }
            }
        }
        ui.slider_config("Sky Brightness", 0.0_f32, 32.0_f32).display_format("%.2f").build(sky_brightness);
        ui.checkbox("Emissive NEE", emissive_nee_enabled);
        ui.checkbox("Analytic NEE", analytic_nee_enabled);
    }

    pub fn build_sharc_section(ui: &imgui::Ui, sharc_mode: &mut RtSharcMode, sharc_scene_scale: &mut f32) {
        if let Some(_combo) = ui.begin_combo("SHARC", sharc_mode.label()) {
            for mode in RtSharcMode::ALL {
                if ui.selectable_config(mode.label()).selected(*sharc_mode == mode).build() {
                    // UI 只更新 mode；缓存 buffer 的生命周期与清零由 pipeline owner 负责。
                    *sharc_mode = mode;
                }
            }
        }
        // scene scale 控制 voxel 物理尺寸，需按场景单位调；第八阶段不查询，只影响缓存粒度与 debug 可视化。
        ui.slider_config("SHARC scene scale", 1.0_f32, 500.0_f32).display_format("%.1f").build(sharc_scene_scale);
    }

    pub fn build_restir_section(ui: &imgui::Ui, restir_di_mode: &mut RtRestirDiMode) {
        if let Some(_combo) = ui.begin_combo("ReSTIR DI", restir_di_mode.label()) {
            for mode in RtRestirDiMode::ALL {
                if ui.selectable_config(mode.label()).selected(*restir_di_mode == mode).build() {
                    // UI 只更新 pipeline mode；跨 mode 的 history 切断由 RenderGraph 构图时
                    // 比较上一帧 mode 完成，避免控件层直接持有 temporal resource 状态。
                    *restir_di_mode = mode;
                }
            }
        }
    }

    pub fn build_tone_mapping_section(
        ui: &imgui::Ui,
        tone_mapping: &mut app_render_passes::sdr_pass::SdrToneMappingSettings,
    ) {
        ui.separator();
        ui.text("Tone Mapping");
        ui.slider_config("Exposure EV", -8.0_f32, 8.0_f32).display_format("%.2f").build(&mut tone_mapping.exposure_ev);
        ui.slider_config("ACES Strength", 0.0_f32, 1.0_f32)
            .display_format("%.2f")
            .build(&mut tone_mapping.aces_strength);
        ui.slider_config("White Point", 1.0_f32, 32.0_f32)
            .display_format("%.2f")
            .build(&mut tone_mapping.aces_white_point);
    }

    fn offline_supports_debug_channel(channel: RtDebugChannel) -> bool {
        !matches!(
            channel,
            RtDebugChannel::RestirInitialWeight
                | RtDebugChannel::RestirTemporalValid
                | RtDebugChannel::RestirFinalContribution
                | RtDebugChannel::SpecularMotionMagnitude
                // SHARC 只在 realtime 主流程维护，离线 raygen 不绑定 / 不维护缓存。
                | RtDebugChannel::SharcHashGrid
                | RtDebugChannel::SharcCache
                | RtDebugChannel::SharcQueryDepth
        )
    }
}
