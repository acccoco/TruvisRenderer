use std::env;

use ash::vk::{self, Handle};
use renderer_rendering::{DlssFeature, DlssFrameSnapshot, DlssOptions, DlssSrFrameConstants, DlssSrMode, DlssSrState};
use truvis_render_foundation::render_view::RenderView;
use truvis_render_runtime::render_runtime_ctx::{
    RenderFrameInput, RenderRuntimeInitCtx, RenderRuntimeResizeCtx, RenderRuntimeUpdateCtx,
};
use truvis_render_runtime::state::frame_state::FrameRenderState;
use truvis_streamline_binding::{StreamlineError, dlss};
use truvis_utils::ConfigUtils;

pub(crate) struct TruvisDlssState {
    requested: DlssOptions,
    effective: DlssOptions,
    sr_supported: bool,
    rr_supported: bool,
    /// 记录可能由 evaluate 创建的 feature；无 TLAS 时可能跳过 pass，不代表已经实际分配。
    feature_to_release: Option<DlssFeature>,
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
            feature_to_release: None,
            sr_state: DlssSrState::default(),
            current_snapshot: DlssFrameSnapshot {
                options: DlssOptions::NATIVE,
                constants: DlssSrFrameConstants::default(),
            },
        }
    }
}

impl TruvisDlssState {
    pub fn init(&mut self, ctx: &mut RenderRuntimeInitCtx<'_>, enabled: bool) -> Result<(), StreamlineError> {
        self.requested = Self::read_options();
        let physical_device = ctx.device_info_ctx.physical_device().vk_handle().as_raw();
        self.sr_supported = dlss::query_support(physical_device).map(|value| value.supported).unwrap_or(false);
        self.rr_supported = dlss::query_rr_support(physical_device).map(|value| value.supported).unwrap_or(false);
        self.effective = self.effective_options(enabled);
        let output_extent = ctx.frame_state.output_extent;
        self.reconfigure(output_extent)?;
        let extent = self.optimal_render_extent(output_extent);
        ctx.set_render_extent(extent);
        Ok(())
    }

    pub fn options_mut(&mut self) -> &mut DlssOptions {
        &mut self.requested
    }

    /// 返回 true 仅表示 update 已完成配置变更，Renderer-owned temporal state 也必须 reset。
    /// 尺寸变化交给随后 Runtime 的 idle/resize 边界处理，避免同帧重复释放、配置和 reset。
    pub fn update(&mut self, ctx: &mut RenderRuntimeUpdateCtx<'_>, enabled: bool) -> Result<bool, StreamlineError> {
        let previous = self.effective;
        self.effective = self.effective_options(enabled);
        let extent = self.optimal_render_extent(ctx.swapchain_extent);
        ctx.set_render_extent(extent);
        if extent != ctx.frame_state.render_extent || previous == self.effective {
            return Ok(false);
        }
        ctx.wait_idle();
        self.reconfigure(ctx.swapchain_extent)?;
        Ok(true)
    }

    pub fn resize(&mut self, ctx: &mut RenderRuntimeResizeCtx<'_>) -> Result<(), StreamlineError> {
        let output_extent = ctx.present.swapchain_image_info().image_extent;
        // RenderRuntime 在生成 ResizeCtx 前已经等待 GPU idle；这里可以直接释放旧 feature。
        self.reconfigure(output_extent)?;
        ctx.set_render_extent(self.optimal_render_extent(output_extent));
        Ok(())
    }

    /// RR options 同时承载配置和逐帧相机矩阵，必须在本帧 view 确定后、evaluate 前提交。
    /// 更新矩阵不释放 feature，也不重置历史；mode/extent 的资源变更仍只发生在 idle 边界。
    pub fn frame_input(
        &mut self,
        render_view: RenderView,
        frame_state: &FrameRenderState,
    ) -> Result<RenderFrameInput, StreamlineError> {
        self.sr_state.update(&render_view, frame_state, self.effective.is_dlss_active());
        let constants = self.sr_state.constants();
        self.current_snapshot = DlssFrameSnapshot {
            options: self.effective,
            constants,
        };
        if self.effective.is_rr_active() {
            dlss::set_rr_options(0, self.streamline_rr_options(frame_state.output_extent))?;
        }
        Ok(RenderFrameInput {
            render_view,
            previous_view: self.sr_state.motion_vector_previous_view().unwrap_or(render_view),
            temporal_jitter_px: constants.sampling_jitter_offset,
        })
    }

    /// evaluate 可能创建 feature；记录其归属，供下一次 mode/resize/shutdown 在 GPU idle 边界释放。
    pub fn snapshot(&mut self, history_reset: bool) -> DlssFrameSnapshot {
        let mut snapshot = self.current_snapshot;
        snapshot.constants.reset |= history_reset;
        self.feature_to_release = snapshot.options.active_feature();
        snapshot
    }

    pub fn reset(&mut self) {
        self.sr_state.request_reset();
    }

    pub fn shutdown(&mut self, ctx: &mut truvis_render_runtime::render_runtime_ctx::RenderRuntimeShutdownCtx<'_>) {
        if self.feature_to_release.is_some() {
            ctx.device_ctx.device().wait_idle();
            self.release_feature();
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

    fn effective_options(&self, enabled: bool) -> DlssOptions {
        if !enabled {
            return DlssOptions::NATIVE;
        }
        let mut options = self.requested;
        if !self.sr_supported {
            options = DlssOptions::NATIVE;
        } else if options.dlss_rr_enabled && !self.rr_supported {
            options.dlss_rr_enabled = false;
        }
        options
    }

    fn optimal_render_extent(&self, output_extent: vk::Extent2D) -> vk::Extent2D {
        if matches!(self.effective.dlss_sr_mode, DlssSrMode::Off | DlssSrMode::Dlaa) {
            return output_extent;
        }
        let result = if self.effective.is_rr_active() {
            dlss::get_rr_optimal_settings(self.streamline_rr_options(output_extent))
        } else {
            dlss::get_optimal_settings(self.streamline_sr_options(output_extent))
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

    /// 仅在初始化或 GPU idle 边界调用；提交失败向上传播，禁止继续以未生效配置渲染。
    fn reconfigure(&mut self, output_extent: vk::Extent2D) -> Result<(), StreamlineError> {
        self.release_feature();
        if self.effective.is_dlss_active() {
            dlss::set_options(0, self.streamline_sr_options(output_extent))?;
        }
        self.sr_state.request_reset();
        Ok(())
    }

    fn streamline_sr_options(&self, output_extent: vk::Extent2D) -> dlss::DlssOptions {
        dlss::DlssOptions {
            mode: self.effective.dlss_sr_mode.to_streamline_mode(),
            output_width: output_extent.width,
            output_height: output_extent.height,
            color_buffers_hdr: true,
        }
    }

    fn streamline_rr_options(&self, output_extent: vk::Extent2D) -> dlss::DlssRrOptions {
        dlss::DlssRrOptions {
            mode: self.effective.dlss_sr_mode.to_streamline_mode(),
            output_width: output_extent.width,
            output_height: output_extent.height,
            color_buffers_hdr: true,
            normal_roughness_packed: true,
            world_to_camera_view: self.current_snapshot.constants.world_to_camera_view,
            camera_view_to_world: self.current_snapshot.constants.camera_view_to_world,
        }
    }

    fn release_feature(&mut self) {
        let Some(feature) = self.feature_to_release.take() else {
            return;
        };
        let result = match feature {
            DlssFeature::SuperResolution => dlss::free_resources(0),
            DlssFeature::RayReconstruction => dlss::free_rr_resources(0),
        };
        if let Err(error) = result {
            log::warn!("failed to release DLSS resources: {error}");
        }
    }
}
