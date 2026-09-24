use renderer_rendering::offline::OfflineRenderSettings;
use renderer_rendering::realtime::{RealtimeRenderSettings, RtRestirDiMode, RtSharcMode};
use renderer_rendering::shared::{
    ColorGradingSettings, ExposureMode, MeteringMode, PathTracingCommonSettings, PathTracingDebugChannel, RenderMode,
    SdrPostProcessSettings, SkySamplingMode, ToneMappingMode,
};
use renderer_rendering::DlssOptions;
use renderer_rendering::DlssSrMode;

#[derive(Default)]
pub struct RenderControlsOverlay;

impl RenderControlsOverlay {
    pub fn build_render_mode_section(ui: &imgui::Ui, render_mode: &mut RenderMode, offline_sample_count: u32) {
        if let Some(_combo) = ui.begin_combo("Render Mode", render_mode.label()) {
            for mode in RenderMode::ALL {
                if ui.selectable_config(mode.label()).selected(*render_mode == mode).build() {
                    *render_mode = mode;
                }
            }
        }

        if *render_mode == RenderMode::Offline {
            ui.text(format!("Offline Samples: {offline_sample_count}"));
        }
    }

    pub fn build_dlss_section_for_mode(ui: &imgui::Ui, render_mode: RenderMode, dlss_options: &mut DlssOptions) {
        // DLSS SR/RR 依赖 realtime 渲染子系统产出的 GBuffer、motion vector 和历史资源；
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

    pub fn build_sampling_section(
        ui: &imgui::Ui,
        render_mode: RenderMode,
        common_settings: &mut PathTracingCommonSettings,
        offline_settings: &mut OfflineRenderSettings,
    ) {
        ui.checkbox("Emissive NEE", &mut common_settings.emissive_nee_enabled);
        ui.checkbox("Analytic NEE", &mut common_settings.analytic_nee_enabled);
        if render_mode == RenderMode::Offline {
            let mut ray_dispatch_count = offline_settings.effective_ray_dispatch_count() as i32;
            if ui
                .slider_config(
                    "Dispatches / Frame",
                    OfflineRenderSettings::MIN_RAY_DISPATCH_COUNT as i32,
                    OfflineRenderSettings::MAX_RAY_DISPATCH_COUNT as i32,
                )
                .display_format("%d")
                .build(&mut ray_dispatch_count)
            {
                offline_settings.set_ray_dispatch_count(ray_dispatch_count as u32);
            }
        }
    }

    pub fn build_realtime_settings_section(
        ui: &imgui::Ui,
        render_mode: RenderMode,
        settings: &mut RealtimeRenderSettings,
    ) {
        let realtime_mode = render_mode == RenderMode::Realtime;
        ui.disabled(!realtime_mode, || {
            Self::build_restir_section(ui, &mut settings.restir_di_mode);
            Self::build_sharc_section(ui, &mut settings.sharc_mode, &mut settings.sharc_scene_scale);
        });
        if !realtime_mode {
            ui.text_disabled("ReSTIR DI and SHARC are realtime only");
        }
    }

    pub fn build_debug_channel_section(
        ui: &imgui::Ui,
        render_mode: RenderMode,
        realtime_settings: &mut RealtimeRenderSettings,
        offline_settings: &mut OfflineRenderSettings,
    ) {
        let selected_channel = match render_mode {
            RenderMode::Realtime => &mut realtime_settings.debug_channel,
            RenderMode::Offline => &mut offline_settings.debug_channel,
        };
        if let Some(_combo) = ui.begin_combo("RT Debug", selected_channel.label()) {
            for channel in PathTracingDebugChannel::ALL {
                if render_mode == RenderMode::Offline && !OfflineRenderSettings::supports_debug_channel(channel) {
                    continue;
                }
                if ui.selectable_config(channel.label()).selected(*selected_channel == channel).build() {
                    *selected_channel = channel;
                }
            }
        }
    }

    pub fn build_sky_section(ui: &imgui::Ui, sky_sampling_mode: &mut SkySamplingMode, sky_brightness: &mut f32) {
        if let Some(_combo) = ui.begin_combo("Sky Sampling", sky_sampling_mode.label()) {
            for mode in SkySamplingMode::ALL {
                if ui.selectable_config(mode.label()).selected(*sky_sampling_mode == mode).build() {
                    *sky_sampling_mode = mode;
                }
            }
        }
        ui.slider_config("Sky Brightness", 0.0_f32, 32.0_f32).display_format("%.2f").build(sky_brightness);
    }

    pub fn build_sharc_section(ui: &imgui::Ui, sharc_mode: &mut RtSharcMode, sharc_scene_scale: &mut f32) {
        if let Some(_combo) = ui.begin_combo("SHARC", sharc_mode.label()) {
            for mode in RtSharcMode::ALL {
                if ui.selectable_config(mode.label()).selected(*sharc_mode == mode).build() {
                    // UI 只更新 mode；缓存 buffer 的生命周期与清零由 realtime 渲染子系统负责。
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
                    // UI 只更新 realtime 渲染模式；跨 mode 的 history 切断由 RenderGraph 构图时
                    // 比较上一帧 mode 完成，避免控件层直接持有 temporal resource 状态。
                    *restir_di_mode = mode;
                }
            }
        }
    }

    pub fn build_post_process_section(ui: &imgui::Ui, post_process: &mut SdrPostProcessSettings) {
        ui.text("Exposure");
        let exposure = &mut post_process.exposure;
        for (mode, label) in [
            (ExposureMode::Auto, "Auto Exposure"),
            (ExposureMode::Manual, "Manual Exposure"),
        ] {
            if ui.radio_button_bool(label, exposure.mode == mode) {
                exposure.mode = mode;
            }
            ui.same_line();
        }
        ui.new_line();
        if exposure.mode == ExposureMode::Manual {
            ui.slider_config("Manual gain (stops)", -24.0_f32, 24.0_f32)
                .display_format("%.2f")
                .build(&mut exposure.manual_ev);
        } else {
            ui.slider_config("Compensation (stops)", -8.0_f32, 8.0_f32)
                .display_format("%.2f")
                .build(&mut exposure.auto_compensation_ev);
            ui.checkbox("Lock auto exposure", &mut exposure.locked);
            for (mode, label) in [
                (MeteringMode::CenterWeighted, "Center weighted"),
                (MeteringMode::Average, "Whole image"),
            ] {
                if ui.radio_button_bool(label, exposure.metering == mode) {
                    exposure.metering = mode;
                }
                ui.same_line();
            }
            ui.new_line();
            ui.slider("Min gain (stops)", -24.0, 24.0, &mut exposure.min_ev);
            ui.slider("Max gain (stops)", -24.0, 24.0, &mut exposure.max_ev);
            ui.slider("Low percentile", 0.0, 1.0, &mut exposure.low_percentile);
            ui.slider("High percentile", 0.0, 1.0, &mut exposure.high_percentile);
            ui.slider("Darken time (s)", 0.01, 10.0, &mut exposure.darken_seconds);
            ui.slider("Brighten time (s)", 0.01, 10.0, &mut exposure.brighten_seconds);
            ui.text_wrapped("Realtime adapts over time; Offline meters the accumulated image directly. Debug channels freeze Final metering. Positive stops brighten the image.");
        }

        ui.separator();
        ui.text("Color Grading");
        let grading = &mut post_process.color_grading;
        if ui.button("Reset grading to neutral") {
            *grading = ColorGradingSettings::default();
        }
        for (label, min, max, value) in [
            ("Temperature (offset)", -100.0, 100.0, &mut grading.temperature),
            ("Tint (offset)", -100.0, 100.0, &mut grading.tint),
            ("Contrast", 0.5, 1.5, &mut grading.contrast),
            ("Shadows (stops)", -2.0, 2.0, &mut grading.shadows_ev),
            ("Midtones (stops)", -2.0, 2.0, &mut grading.midtones_ev),
            ("Highlights (stops)", -2.0, 2.0, &mut grading.highlights_ev),
            ("Saturation", 0.0, 2.0, &mut grading.saturation),
        ] {
            ui.slider_config(label, min, max).display_format("%.2f").build(value);
        }
        ui.text_wrapped("Positive Temperature warms; positive Tint shifts toward magenta. Tonal gains preserve black.");
        ui.separator();
        ui.text("Tone Mapping");
        for (mode, label) in [
            (ToneMappingMode::AcesFitted, "ACES fitted"),
            (ToneMappingMode::AgX, "AgX"),
            (ToneMappingMode::PbrNeutral, "Khronos PBR Neutral"),
            (ToneMappingMode::None, "None"),
        ] {
            if ui.radio_button_bool(label, post_process.tone_mapping == mode) {
                post_process.tone_mapping = mode;
            }
        }
        ui.checkbox("Dithering", &mut post_process.dither);
    }
}
