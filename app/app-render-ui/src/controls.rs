use app_rendering::offline::OfflineRenderSettings;
use app_rendering::realtime::{RealtimeRenderSettings, RtRestirDiMode, RtSharcMode};
use app_rendering::shared::{
    PathTracingCommonSettings, PathTracingDebugChannel, RenderMode, SdrToneMappingSettings, SkySamplingMode,
};
use truvis_render_runtime::state::dlss_options::DlssOptions;
use truvis_render_runtime::state::dlss_sr::DlssSrMode;

#[derive(Default)]
pub struct RenderControlsOverlay;

impl RenderControlsOverlay {
    /// Cornell 等只有 realtime owner 的 Renderer 使用专用窗口，不虚构不存在的 offline 状态。
    pub fn build_realtime_window(
        &mut self,
        ui: &imgui::Ui,
        dlss_options: &mut DlssOptions,
        common_settings: &mut PathTracingCommonSettings,
        realtime_settings: &mut RealtimeRenderSettings,
    ) {
        ui.window("Controls")
            .position([10.0, 200.0], imgui::Condition::FirstUseEver)
            .size([340.0, 360.0], imgui::Condition::FirstUseEver)
            .build(|| {
                Self::build_dlss_section(ui, dlss_options);
                ui.separator();
                Self::build_realtime_section(ui, common_settings, realtime_settings);
            });
    }

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

    pub fn build_mode_specific_sections(
        ui: &imgui::Ui,
        render_mode: RenderMode,
        common_settings: &mut PathTracingCommonSettings,
        realtime_settings: &mut RealtimeRenderSettings,
        offline_settings: &mut OfflineRenderSettings,
    ) {
        match render_mode {
            RenderMode::Realtime => Self::build_realtime_section(ui, common_settings, realtime_settings),
            RenderMode::Offline => {
                Self::build_offline_section(ui, common_settings, offline_settings);
                ui.disabled(true, || {
                    Self::build_restir_section(ui, &mut realtime_settings.restir_di_mode);
                });
                ui.text_disabled("ReSTIR DI is realtime only");
            }
        }
    }

    pub fn build_realtime_section(
        ui: &imgui::Ui,
        common_settings: &mut PathTracingCommonSettings,
        realtime_settings: &mut RealtimeRenderSettings,
    ) {
        if let Some(_combo) = ui.begin_combo("RT debug", realtime_settings.debug_channel.label()) {
            for channel in PathTracingDebugChannel::ALL {
                if ui.selectable_config(channel.label()).selected(realtime_settings.debug_channel == channel).build() {
                    realtime_settings.debug_channel = channel;
                }
            }
        }
        Self::build_common_sampling_section(
            ui,
            &mut common_settings.sky_sampling_mode,
            &mut common_settings.sky_brightness,
            &mut common_settings.emissive_nee_enabled,
            &mut common_settings.analytic_nee_enabled,
        );
        Self::build_restir_section(ui, &mut realtime_settings.restir_di_mode);
        Self::build_sharc_section(ui, &mut realtime_settings.sharc_mode, &mut realtime_settings.sharc_scene_scale);
        Self::build_tone_mapping_section(ui, &mut common_settings.tone_mapping);
    }

    pub fn build_offline_section(
        ui: &imgui::Ui,
        common_settings: &mut PathTracingCommonSettings,
        offline_settings: &mut OfflineRenderSettings,
    ) {
        // 离线 raygen 不维护 ReSTIR reservoir，也不执行 realtime ReSTIR phase。若离线设置来自旧配置
        // 或未来入口而落在 ReSTIR-only debug channel，这里先收敛到 Final，避免展示无来源图像。
        if !Self::offline_supports_debug_channel(offline_settings.debug_channel) {
            offline_settings.debug_channel = PathTracingDebugChannel::Final;
        }

        if let Some(_combo) = ui.begin_combo("RT debug", offline_settings.debug_channel.label()) {
            for channel in PathTracingDebugChannel::ALL {
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
            OfflineRenderSettings::MIN_RAY_DISPATCH_COUNT as i32,
            OfflineRenderSettings::MAX_RAY_DISPATCH_COUNT as i32,
        )
        .display_format("%d")
        .build(&mut ray_dispatch_count);
        offline_settings.set_ray_dispatch_count(ray_dispatch_count as u32);
        Self::build_common_sampling_section(
            ui,
            &mut common_settings.sky_sampling_mode,
            &mut common_settings.sky_brightness,
            &mut common_settings.emissive_nee_enabled,
            &mut common_settings.analytic_nee_enabled,
        );
        Self::build_tone_mapping_section(ui, &mut common_settings.tone_mapping);
    }

    pub fn build_common_sampling_section(
        ui: &imgui::Ui,
        sky_sampling_mode: &mut SkySamplingMode,
        sky_brightness: &mut f32,
        emissive_nee_enabled: &mut bool,
        analytic_nee_enabled: &mut bool,
    ) {
        if let Some(_combo) = ui.begin_combo("Sky sampling", sky_sampling_mode.label()) {
            for mode in SkySamplingMode::ALL {
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

    pub fn build_tone_mapping_section(ui: &imgui::Ui, tone_mapping: &mut SdrToneMappingSettings) {
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

    fn offline_supports_debug_channel(channel: PathTracingDebugChannel) -> bool {
        !matches!(
            channel,
            PathTracingDebugChannel::RestirInitialWeight
                | PathTracingDebugChannel::RestirTemporalValid
                | PathTracingDebugChannel::RestirFinalContribution
                | PathTracingDebugChannel::SpecularMotionMagnitude
                // SHARC 只在 realtime 主流程维护，离线 raygen 不绑定 / 不维护缓存。
                | PathTracingDebugChannel::SharcHashGrid
                | PathTracingDebugChannel::SharcCache
                | PathTracingDebugChannel::SharcQueryDepth
        )
    }
}
