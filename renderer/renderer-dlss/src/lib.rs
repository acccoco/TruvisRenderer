//! Renderer 主视图的 DLSS 配置、逐帧录制和资源生命周期。

use ash::vk::{self, Handle};
use truvis_render_foundation::render_view::RenderView;
use truvis_render_runtime::{
    render_runtime_ctx::{
        RenderFrameInput, RenderRuntimeInitCtx, RenderRuntimeResizeCtx, RenderRuntimeShutdownCtx,
        RenderRuntimeUpdateCtx,
    },
    state::{frame_state::FrameRenderState, frame_timing::FrameTiming},
};
use truvis_streamline_binding::{StreamlineError, dlss};

mod frame;
mod options;
mod resources;
mod temporal;

pub use frame::{DlssFrame, DlssInputs, DlssRrGuides};
use options::DlssFeature;
pub use options::{DlssOptions, DlssSrMode};
use resources::DlssTargets;
use temporal::DlssSrState;

const VIEWPORT_ID: u32 = 0;

/// release 错误可能发生在部分释放后；保留 feature 归属，禁止把再次调用当成恢复措施。
enum DlssFeatureState
{
    Unused,
    Recorded(DlssFeature),
    ReleaseFailed(DlssFeature),
}

/// 由 Renderer 在 RenderThread 串行持有。GPU 图像通过 resize/shutdown ctx 显式释放。
pub struct Dlss
{
    requested: DlssOptions,
    effective: DlssOptions,
    sr_supported: bool,
    rr_supported: bool,
    /// 记录已开始录制的 DLSS graph 可能创建的 feature，不代表 SDK 已成功分配。
    feature_state: DlssFeatureState,
    sr_state: DlssSrState,
    targets: Option<DlssTargets>,
}

impl Dlss
{
    pub fn new(motion_vectors_invalid_value: f32) -> Self
    {
        Self {
            requested: DlssOptions::NATIVE,
            effective: DlssOptions::NATIVE,
            sr_supported: false,
            rr_supported: false,
            feature_state: DlssFeatureState::Unused,
            sr_state: DlssSrState::new(motion_vectors_invalid_value),
            targets: None,
        }
    }

    pub fn init(
        &mut self,
        ctx: &mut RenderRuntimeInitCtx<'_>,
        options: DlssOptions,
        enabled: bool,
    ) -> Result<(), StreamlineError>
    {
        assert!(self.targets.is_none(), "DLSS already initialized");
        self.requested = options;
        let physical_device = ctx.device_info_ctx.physical_device().vk_handle().as_raw();
        // SAFETY: Renderer init 在 RenderThread 串行执行，ctx 提供当前存活 Gfx 的 physical device。
        let (sr_support, rr_support) =
            unsafe { (dlss::query_support(physical_device), dlss::query_rr_support(physical_device)) };
        self.sr_supported = sr_support.map(|value| value.supported).unwrap_or(false);
        self.rr_supported = rr_support.map(|value| value.supported).unwrap_or(false);
        self.effective = self.effective_options(enabled);
        let output_extent = ctx.frame_state.output_extent;
        self.reconfigure(output_extent)?;
        let extent = self.optimal_render_extent(output_extent);
        ctx.set_render_extent(extent);
        self.targets = Some(DlssTargets::new(ctx));
        Ok(())
    }

    pub fn options_mut(&mut self) -> &mut DlssOptions
    {
        &mut self.requested
    }

    /// 返回 true 仅表示 update 已完成配置变更，Renderer-owned temporal state 也必须 reset。
    /// 尺寸变化交给随后 Runtime 的 idle/resize 边界处理，避免同帧重复释放、配置和 reset。
    pub fn update(&mut self, ctx: &mut RenderRuntimeUpdateCtx<'_>, enabled: bool) -> Result<bool, StreamlineError>
    {
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

    pub fn resize(&mut self, ctx: &mut RenderRuntimeResizeCtx<'_>) -> Result<(), StreamlineError>
    {
        let output_extent = ctx.present.swapchain_image_info().image_extent;
        // RenderRuntime 在生成 ResizeCtx 前已经等待 GPU idle；这里可以直接释放旧 feature。
        self.reconfigure(output_extent)?;
        ctx.set_render_extent(self.optimal_render_extent(output_extent));
        self.targets.as_mut().expect("DLSS targets not initialized").resize(ctx);
        Ok(())
    }

    /// RR options 同时承载配置和逐帧相机矩阵，必须在本帧 view 确定后、evaluate 前提交。
    /// 更新矩阵不释放 feature，也不重置历史；mode/extent 的资源变更仍只发生在 idle 边界。
    pub fn frame_input(
        &mut self,
        render_view: RenderView,
        frame_state: &FrameRenderState,
    ) -> Result<RenderFrameInput, StreamlineError>
    {
        self.sr_state.update(&render_view, frame_state, self.effective.is_dlss_active());
        let constants = self.sr_state.constants();
        if self.effective.is_rr_active() {
            // SAFETY: RenderThread 在 evaluate 前提交本帧矩阵，extent 重建已越过 idle 边界，guide 固定 packed。
            unsafe { dlss::set_rr_options(VIEWPORT_ID, self.streamline_rr_options(frame_state.output_extent))? };
        }
        Ok(RenderFrameInput {
            render_view,
            previous_view: self.sr_state.motion_vector_previous_view().unwrap_or(render_view),
            temporal_jitter_px: constants.sampling_jitter_offset,
        })
    }

    /// 冻结 prepare 与 after_prepare 得出的本帧输入。图像句柄仅用于当前帧录制。
    pub fn frame(&self, timing: &FrameTiming) -> DlssFrame
    {
        let targets = self.targets.as_ref().expect("DLSS targets not initialized");
        DlssFrame::new(
            timing.frame_id() as u32,
            self.effective.active_feature(),
            self.sr_state.constants(),
            targets.output(timing.frame_label()),
            targets.exposure(),
        )
    }

    /// 在 graph 执行前登记可能创建的 feature，即使录制后来失败也保留释放责任。
    pub fn begin_recording(&mut self, frame: &DlssFrame)
    {
        let Some(feature) = frame.begin_recording() else {
            return;
        };
        match self.feature_state {
            DlssFeatureState::Unused => {}
            DlssFeatureState::Recorded(previous) => {
                assert_eq!(previous, feature, "DLSS feature must be released before recording a different feature")
            }
            DlssFeatureState::ReleaseFailed(feature) => {
                panic!("cannot record DLSS {feature:?} after release failed");
            }
        }
        self.feature_state = DlssFeatureState::Recorded(feature);
    }

    pub fn reset(&mut self)
    {
        self.sr_state.request_reset();
    }

    /// 只在 queue submit 正常返回后确认时域状态。
    pub fn finish_rendered_frame(&mut self, scene_submitted: bool, frame: DlssFrame)
    {
        self.sr_state.finish_rendered_frame(scene_submitted, frame.evaluated_for_submit());
    }

    pub fn shutdown(&mut self, ctx: &mut RenderRuntimeShutdownCtx<'_>) -> Result<(), StreamlineError>
    {
        // RenderLoop 在交付 ShutdownCtx 前已等待 GPU idle，旧图像及 feature 都可以释放。
        self.release_feature()?;
        if let Some(targets) = self.targets.take() {
            targets.destroy(ctx);
        }
        Ok(())
    }

    fn effective_options(&self, enabled: bool) -> DlssOptions
    {
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

    fn optimal_render_extent(&self, output_extent: vk::Extent2D) -> vk::Extent2D
    {
        if matches!(self.effective.dlss_sr_mode, DlssSrMode::Off | DlssSrMode::Dlaa) {
            return output_extent;
        }
        // SAFETY: 本状态仅由 RenderThread 在 Gfx/Streamline 存活期串行访问，查询不会与初始化或关闭竞争。
        let result = unsafe {
            if self.effective.is_rr_active() {
                dlss::get_rr_optimal_settings(self.streamline_rr_options(output_extent))
            } else {
                dlss::get_optimal_settings(self.streamline_sr_options(output_extent))
            }
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
    fn reconfigure(&mut self, output_extent: vk::Extent2D) -> Result<(), StreamlineError>
    {
        self.release_feature()?;
        if self.effective.is_dlss_active() {
            // SAFETY: 仅从 init/idle 后的 update/resize 调用；GPU 已完成，新 extent 用于随后构建的图像。
            unsafe { dlss::set_options(VIEWPORT_ID, self.streamline_sr_options(output_extent))? };
        }
        self.sr_state.reconfigure();
        Ok(())
    }

    fn streamline_sr_options(&self, output_extent: vk::Extent2D) -> dlss::DlssOptions
    {
        dlss::DlssOptions {
            mode: self.effective.dlss_sr_mode.to_streamline_mode(),
            output_width: output_extent.width,
            output_height: output_extent.height,
            color_buffers_hdr: true,
        }
    }

    fn streamline_rr_options(&self, output_extent: vk::Extent2D) -> dlss::DlssRrOptions
    {
        dlss::DlssRrOptions {
            mode: self.effective.dlss_sr_mode.to_streamline_mode(),
            output_width: output_extent.width,
            output_height: output_extent.height,
            color_buffers_hdr: true,
            normal_roughness_packed: true,
            world_to_camera_view: self.sr_state.constants().world_to_camera_view,
            camera_view_to_world: self.sr_state.constants().camera_view_to_world,
        }
    }

    fn release_feature(&mut self) -> Result<(), StreamlineError>
    {
        let feature = match &self.feature_state {
            DlssFeatureState::Unused => return Ok(()),
            DlssFeatureState::Recorded(feature) => *feature,
            DlssFeatureState::ReleaseFailed(feature) => panic!("cannot retry failed DLSS {feature:?} release"),
        };
        // SAFETY: reconfigure/shutdown 在 RenderThread 的 GPU idle 边界调用，旧 graph 不再提交，Gfx 仍存活。
        let result = unsafe {
            match feature {
                DlssFeature::SuperResolution => dlss::free_resources(VIEWPORT_ID),
                DlssFeature::RayReconstruction => dlss::free_rr_resources(VIEWPORT_ID),
            }
        };
        match result {
            Ok(()) => {
                self.feature_state = DlssFeatureState::Unused;
                Ok(())
            }
            Err(error) => {
                self.feature_state = DlssFeatureState::ReleaseFailed(feature);
                Err(error)
            }
        }
    }
}
