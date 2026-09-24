use std::env;

use ash::vk::{self, Handle};
use renderer_rendering::{DlssFeature, DlssFrameSnapshot, DlssOptions, DlssSrFrameConstants, DlssSrMode, DlssSrState};
use truvis_render_foundation::render_view::RenderView;
use truvis_render_runtime::render_runtime_ctx::{
    RenderFrameInput, RenderRuntimeInitCtx, RenderRuntimeResizeCtx, RenderRuntimeUpdateCtx,
};
use truvis_render_runtime::state::frame_state::FrameRenderState;
use truvis_streamline_binding::dlss;
use truvis_utils::ConfigUtils;

pub(crate) struct TruvisDlssState {
    requested: DlssOptions,
    effective: DlssOptions,
    sr_supported: bool,
    rr_supported: bool,
    applied_feature: Option<DlssFeature>,
    sr_state: DlssSrState,
    current_snapshot: DlssFrameSnapshot,
}

impl Default for TruvisDlssState {
    fn default() -> Self {
        Self {
            requested: DlssOptions::NATIVE,
            effective: DlssOptions::NATIVE,
            sr_supported: false,
            rr_supported: false,
            applied_feature: None,
            sr_state: DlssSrState::default(),
            current_snapshot: DlssFrameSnapshot {
                options: DlssOptions::NATIVE,
                constants: DlssSrFrameConstants::default(),
            },
        }
    }
}

impl TruvisDlssState {
    pub fn init(&mut self, ctx: &mut RenderRuntimeInitCtx<'_>) {
        self.requested = Self::read_options();
        let physical_device = ctx.device_info_ctx.physical_device().vk_handle().as_raw();
        self.sr_supported = dlss::query_support(physical_device).map(|value| value.supported).unwrap_or(false);
        self.rr_supported = dlss::query_rr_support(physical_device).map(|value| value.supported).unwrap_or(false);
        self.effective = self.effective_options();
        self.applied_feature = self.effective.active_feature();
        let extent = self.optimal_render_extent(ctx.frame_state.output_extent);
        ctx.set_render_extent(extent);
    }

    pub fn options_mut(&mut self) -> &mut DlssOptions {
        &mut self.requested
    }

    pub fn update(&mut self, ctx: &mut RenderRuntimeUpdateCtx<'_>) {
        let previous = self.effective;
        self.effective = self.effective_options();
        if previous != self.effective {
            self.apply_feature_change(ctx);
        }
        ctx.set_render_extent(self.optimal_render_extent(ctx.swapchain_extent));
    }

    pub fn resize(&mut self, ctx: &mut RenderRuntimeResizeCtx<'_>) {
        let output_extent = ctx.present.swapchain_image_info().image_extent;
        ctx.set_render_extent(self.optimal_render_extent(output_extent));
        self.sr_state.request_reset();
    }

    pub fn frame_input(&mut self, render_view: RenderView, frame_state: &FrameRenderState) -> RenderFrameInput {
        self.sr_state.update(&render_view, frame_state, self.effective.is_dlss_active());
        let constants = self.sr_state.constants();
        self.current_snapshot = DlssFrameSnapshot {
            options: self.effective,
            constants,
        };
        RenderFrameInput {
            render_view,
            previous_view: self.sr_state.motion_vector_previous_view().unwrap_or(render_view),
            temporal_jitter_px: constants.sampling_jitter_offset,
        }
    }

    pub fn snapshot(&self, history_reset: bool) -> DlssFrameSnapshot {
        let mut snapshot = self.current_snapshot;
        snapshot.constants.reset |= history_reset;
        snapshot
    }

    pub fn reset(&mut self) {
        self.sr_state.request_reset();
    }

    pub fn shutdown(&mut self, ctx: &mut truvis_render_runtime::render_runtime_ctx::RenderRuntimeShutdownCtx<'_>) {
        if self.applied_feature.is_some() {
            ctx.device_ctx.device().wait_idle();
            let result = match self.applied_feature.take() {
                Some(DlssFeature::SuperResolution) => dlss::free_resources(0),
                Some(DlssFeature::RayReconstruction) => dlss::free_rr_resources(0),
                None => Ok(()),
            };
            if let Err(error) = result {
                log::warn!("failed to release DLSS resources: {error}");
            }
        }
    }

    fn read_options() -> DlssOptions {
        let mut options = DlssOptions::NATIVE;
        if let Ok(value) = env::var("TRUVIS_DLSS_SR_MODE") {
            if let Some(mode) = DlssSrMode::from_config_value(&value) {
                options.dlss_sr_mode = mode;
            }
        }
        if let Ok(value) = env::var("TRUVIS_DLSS_RR") {
            if let Some(enabled) = ConfigUtils::parse_bool_env(&value) {
                options.dlss_rr_enabled = enabled;
            }
        }
        options
    }

    fn effective_options(&self) -> DlssOptions {
        let mut options = self.requested;
        if !self.sr_supported {
            options = DlssOptions::NATIVE;
        } else if options.dlss_rr_enabled && !self.rr_supported {
            options.dlss_rr_enabled = false;
        }
        options
    }

    fn optimal_render_extent(&mut self, output_extent: vk::Extent2D) -> vk::Extent2D {
        if matches!(self.effective.dlss_sr_mode, DlssSrMode::Off | DlssSrMode::Dlaa) {
            return output_extent;
        }
        let mode = self.effective.dlss_sr_mode.to_streamline_mode();
        let result = if self.effective.is_rr_active() {
            dlss::get_rr_optimal_settings(dlss::DlssRrOptions {
                mode,
                output_width: output_extent.width,
                output_height: output_extent.height,
                color_buffers_hdr: true,
                normal_roughness_packed: true,
                world_to_camera_view: Self::IDENTITY,
                camera_view_to_world: Self::IDENTITY,
            })
        } else {
            dlss::get_optimal_settings(dlss::DlssOptions {
                mode,
                output_width: output_extent.width,
                output_height: output_extent.height,
                color_buffers_hdr: true,
            })
        };
        match result {
            Ok(settings) if settings.optimal_render_width > 0 && settings.optimal_render_height > 0 => vk::Extent2D {
                width: settings.optimal_render_width,
                height: settings.optimal_render_height,
            },
            Ok(settings) => {
                log::warn!(
                    "DLSS optimal settings returned invalid extent {}x{}; using native extent",
                    settings.optimal_render_width,
                    settings.optimal_render_height
                );
                output_extent
            }
            Err(error) => {
                log::warn!("DLSS optimal settings query failed: {error}; using native extent");
                output_extent
            }
        }
    }

    fn apply_feature_change(&mut self, ctx: &RenderRuntimeUpdateCtx<'_>) {
        let next_feature = self.effective.active_feature();
        if self.applied_feature != next_feature {
            ctx.wait_idle();
            if let Some(feature) = self.applied_feature.take() {
                let result = match feature {
                    DlssFeature::SuperResolution => dlss::free_resources(0),
                    DlssFeature::RayReconstruction => dlss::free_rr_resources(0),
                };
                if let Err(error) = result {
                    log::warn!("failed to release DLSS resources: {error}");
                }
            }
            self.applied_feature = next_feature;
        }
        self.sr_state.request_reset();
    }

    const IDENTITY: [f32; 16] = [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ];
}
