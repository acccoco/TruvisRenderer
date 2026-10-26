//! NR 会话和窗口尺寸资源的唯一 owner；组织 gamma SDR evaluate 与线性 SDR 解码。
//! requested 保留用户意图，effective 表达当前模式是否允许执行；Running 仅表示成功录制并提交。

use std::{
    cell::{Cell, RefCell},
    os::windows::ffi::OsStrExt,
};

use ash::vk::{self, Handle};

use renderer_kit::subsystem::SubsystemRenderCtx;
pub use renderer_render_passes::post_process::dlss_nr::NrEvaluation;
use renderer_render_passes::post_process::dlss_nr::{NrDecodePass, NrDecodeRgPass, NrGuides, NrRgPass};
use renderer_render_passes::post_process::dlss_options::{DlssEvaluation, DlssFrameSnapshot};
use truvis_dlss_nr_binding::{InitDesc, NrSession};
use truvis_gfx::commands::barrier::GfxImageBarrier;
use truvis_gfx::gfx::{GfxDeviceCtx, GfxResourceCtx};
use truvis_gfx::resources::lifecycle::DestroyReason;
use truvis_path::TruvisPath;
use truvis_render_graph::render_graph::{RenderGraphBuilder, RgImageHandle};
use truvis_render_runtime::render_runtime_ctx::{
    RenderRuntimeInitCtx, RenderRuntimeResourceCtx, RenderRuntimeShutdownCtx,
};
use truvis_render_runtime::resources::gfx_resource_registry::GfxResourceRegistry;

use crate::post_process::{SdrOutputEncoding, SdrPostProcessInput};
use crate::shared::targets::{SingleImageTarget, TargetImageDesc};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NrOptions {
    pub enabled: bool,
    pub intensity: f32,
}

impl Default for NrOptions {
    fn default() -> Self {
        Self {
            enabled: false,
            intensity: 1.0,
        }
    }
}

impl NrOptions {
    pub fn normalize(&mut self) {
        self.intensity = if self.intensity.is_finite() { self.intensity.clamp(0.0, 1.0) } else { 1.0 };
    }
}

/// CPU 展示快照；Ready 只表示资源创建已提交完成，不表示 NR 输出有效。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NrStatus {
    Off,
    Bypassed,
    Pending,
    Ready,
    Running,
}

/// Renderer 持有本类型；NrSession 使它不能跨线程，Ctx 从不保存在成员中。
#[derive(Default)]
pub struct NrSubsystem {
    requested: NrOptions,
    effective: NrOptions,
    session: Option<RefCell<NrSession>>,
    decode: Option<NrDecodePass>,
    submitted_frames: u64,
    resources: Option<NrResources>,
    reset_pending: bool,
    /// 释放失败后保留原始错误，最终清理只能报告并终止，不能重试或关闭共享 NGX。
    release_error: Option<String>,
}

impl NrSubsystem {
    pub fn options_mut(&mut self) -> &mut NrOptions {
        &mut self.requested
    }
    pub fn requested(&self) -> NrOptions {
        self.requested
    }
    pub fn effective(&self) -> NrOptions {
        self.effective
    }
    pub fn reset_pending(&self) -> bool {
        self.reset_pending
    }
    pub fn status(&self) -> NrStatus {
        if !self.requested.enabled {
            NrStatus::Off
        } else if !self.effective.enabled {
            NrStatus::Bypassed
        } else if self.resources.is_some() {
            if self.submitted_frames > 0 { NrStatus::Running } else { NrStatus::Ready }
        } else {
            NrStatus::Pending
        }
    }

    /// 返回是否需要资源回调；强度变化只更新配置，不触发 idle 或重建。
    pub fn update(&mut self, eligible: bool, output_extent: vk::Extent2D) -> bool {
        self.requested.normalize();
        let previous = self.effective;
        self.effective = NrOptions {
            enabled: self.requested.enabled && eligible,
            intensity: self.requested.intensity,
        };
        if previous != self.effective {
            log::info!("NR configuration requested={:?} effective={:?}", self.requested, self.effective);
        }
        if previous.enabled != self.effective.enabled {
            self.request_reset();
        }
        match self.resources.as_ref() {
            Some(resources) => !self.effective.enabled || resources.extent != output_extent,
            None => self.effective.enabled,
        }
    }

    /// 仅本 feature 的 evaluate 成功且 queue submit 返回后才消费，不依赖其它历史的提交状态。
    pub fn request_reset(&mut self) {
        self.reset_pending = true;
        self.submitted_frames = 0;
    }

    /// Runtime 已保证 GPU idle；失败时保留会话/资源供最终 shutdown 按顺序回收。
    pub fn reconfigure(&mut self, ctx: &mut RenderRuntimeResourceCtx<'_>) -> Result<(), String> {
        let extent = ctx.frame_state.output_extent;
        if self.effective.enabled && self.resources.as_ref().is_some_and(|r| r.extent == extent) {
            return Ok(());
        }
        self.release_feature()?;
        self.release_images(ctx.resource_ctx, ctx.device_ctx, ctx.gfx_resource_registry, DestroyReason::Resize);
        self.request_reset();
        if !self.effective.enabled {
            return Ok(());
        }
        if extent.width == 0 || extent.height == 0 {
            return Err("NR cannot create zero-extent resources".into());
        }
        if self.session.is_none() {
            self.init_session(ctx)?;
        }
        NrResources::check_formats(ctx)?;
        self.resources = Some(NrResources::new(ctx));
        let session = self.session.as_mut().unwrap().get_mut();
        ctx.immediate_ctx
            .try_one_time_exec(
                |cmd| unsafe { session.create_feature(cmd.vk_handle().as_raw(), extent.width, extent.height) },
                "nr-create-feature",
            )
            .map_err(|e| e.to_string())?;
        log::info!(
            "NR resources ready: {}x{}, feature create submitted/completed; awaiting evaluation",
            extent.width,
            extent.height
        );
        Ok(())
    }

    /// 路径只同步借用，root/proc 与 Streamline 同源且存活至 shutdown。
    /// 固定 SDK 的 properties2/BDA 由 Vulkan 1.3 提供，NVX/push descriptor 由 interposer 启用；
    /// GDPA 必须在实际 device 上返回对应入口，不能用物理设备支持性代替启用状态。
    fn init_session(&mut self, ctx: &RenderRuntimeResourceCtx<'_>) -> Result<(), String> {
        let executable = std::env::current_exe().map_err(|e| e.to_string())?;
        let directory = executable.parent().ok_or("NR executable directory missing")?;
        let log_dir = TruvisPath::workspace().join(".temp/dlss-nr");
        std::fs::create_dir_all(&log_dir).map_err(|e| e.to_string())?;
        let dll_path: Vec<u16> = directory.as_os_str().encode_wide().chain(Some(0)).collect();
        let log_path: Vec<u16> = log_dir.as_os_str().encode_wide().chain(Some(0)).collect();
        let info = ctx.device_info_ctx;
        let instance = info.instance().ash_instance();
        let device = ctx.device_ctx.device();
        let enabled = device.enabled_features();
        if !(enabled.buffer_device_address
            && enabled.shader_float16
            && enabled.storage_buffer16_bit_access
            && enabled.uniform_and_storage_buffer16_bit_access
            && enabled.synchronization2)
        {
            return Err("NR required features were not enabled at device creation".into());
        }
        let desc = InitDesc {
            instance: instance.handle().as_raw(),
            physical_device: info.physical_device().vk_handle().as_raw(),
            device: device.handle().as_raw(),
            get_instance_proc_addr: info.get_instance_proc_addr() as usize as u64,
            get_device_proc_addr: instance.fp_v1_0().get_device_proc_addr as usize as u64,
            dll_directory_utf16: dll_path.as_ptr(),
            log_directory_utf16: log_path.as_ptr(),
            enabled_features: truvis_dlss_nr_binding::raw::TruvixxNrRequiredFeatures as u32,
        };
        let instance_requirements = unsafe { NrSession::extensions(&desc, false) }.map_err(|e| e.to_string())?;
        let device_requirements = unsafe { NrSession::extensions(&desc, true) }.map_err(|e| e.to_string())?;
        log::info!(
            "NR requirements instance={instance_requirements:?} device={device_requirements:?}; enabled features verified"
        );
        if instance_requirements.iter().any(|s| s != "VK_KHR_get_physical_device_properties2") {
            return Err(format!("NR instance requirements changed: {instance_requirements:?}"));
        }
        for requirement in &device_requirements {
            let proc = match requirement.as_str() {
                "VK_NVX_binary_import" => c"vkCreateCuModuleNVX",
                "VK_NVX_image_view_handle" => c"vkGetImageViewHandleNVX",
                "VK_KHR_push_descriptor" => c"vkCmdPushDescriptorSetKHR",
                "VK_KHR_buffer_device_address" => c"vkGetBufferDeviceAddress",
                _ => return Err(format!("NR device requirement not covered by creation contract: {requirement}")),
            };
            if unsafe { instance.get_device_proc_addr(device.handle(), proc.as_ptr()) }.is_none() {
                return Err(format!("NR required device entry unavailable: {proc:?}"));
            }
        }
        self.session = Some(RefCell::new(unsafe { NrSession::init(&desc) }.map_err(|e| e.to_string())?));
        let caps = self.session.as_mut().unwrap().get_mut().capabilities().map_err(|e| e.to_string())?;
        log::info!("NR capabilities: {caps:?}");
        for q in [
            caps.available,
            caps.needs_updated_driver,
            caps.feature_init_result,
            caps.min_driver_major,
            caps.min_driver_minor,
        ] {
            if q.ngx_result != 1 {
                return Err(format!("NR capability read failed: NGX={:#x}", q.ngx_result));
            }
        }
        if caps.available.value != 1 || caps.needs_updated_driver.value != 0 || caps.feature_init_result.value != 1 {
            return Err(format!("NR unavailable: {caps:?}"));
        }
        log::info!("NR session initialized: {}", directory.display());
        Ok(())
    }

    /// 只能在 GPU idle 后调用；C ABI 对未创建 feature 的 release 是幂等操作。
    pub fn release_feature(&mut self) -> Result<(), String> {
        if let Some(error) = &self.release_error {
            return Err(error.clone());
        }
        if let Some(session) = self.session.as_mut() {
            if let Err(error) = unsafe { session.get_mut().release_feature() } {
                let error = error.to_string();
                self.release_error = Some(error.clone());
                return Err(error);
            }
            log::info!("NR feature released; session retained");
        }
        Ok(())
    }

    fn release_images(
        &mut self,
        resource: GfxResourceCtx<'_>,
        device: GfxDeviceCtx<'_>,
        registry: &mut GfxResourceRegistry,
        reason: DestroyReason,
    ) {
        if let Some(mut resources) = self.resources.take() {
            for image in &mut resources.images {
                image.destroy(resource, device, registry, reason);
            }
            log::info!("NR images released: {reason}");
        }
    }

    /// 调用方已 GPU idle 并成功释放全部 SR/RR 与 NR feature，随后才允许关闭共享 NGX 会话。
    pub fn shutdown(&mut self, ctx: &mut RenderRuntimeShutdownCtx<'_>) -> Result<(), String> {
        if let Some(error) = &self.release_error {
            return Err(error.clone());
        }
        self.release_images(ctx.resource_ctx, ctx.device_ctx, ctx.gfx_resource_registry, DestroyReason::Shutdown);
        if let Some(session) = self.session.as_mut() {
            unsafe { session.get_mut().shutdown() }.map_err(|e| e.to_string())?;
            self.session = None;
            log::info!("NR session shutdown completed");
        }
        if let Some(decode) = self.decode.take() {
            decode.destroy(ctx.device_ctx);
        }
        Ok(())
    }
}

/// 只在当前 compute graph 中有效；原 SDR destination 仍由 Realtime 持有。
pub struct NrGraphFrame {
    color: RgImageHandle,
    mask: RgImageHandle,
    output: RgImageHandle,
    destination: RgImageHandle,
    guides: NrGuides,
    extent: vk::Extent2D,
}

impl NrSubsystem {
    /// pipeline 与其它显示 pass 一起创建；NGX 会话仍只在显式开启时初始化。
    pub fn init(&mut self, ctx: &RenderRuntimeInitCtx<'_>) {
        self.decode = Some(NrDecodePass::new(ctx.device_ctx, ctx.shader_binding_system.global_descriptor_sets()));
    }

    pub fn prepare_graph(
        &self,
        graph: &mut RenderGraphBuilder<'_>,
        display: &mut SdrPostProcessInput,
    ) -> Option<NrGraphFrame> {
        if !self.effective.enabled {
            return None;
        }
        let guides = display.guides?;
        let resources = self.resources.as_ref().expect("effective NR resources missing");
        assert_eq!(resources.extent, display.destination_extent);
        let handles = std::array::from_fn::<_, 3, _>(|index| {
            let image = &resources.images[index];
            let target = image.target();
            graph.import_image(
                NrResources::CHANNELS[index].0, target.image, Some(target.view), target.format, NrRgPass::WRITE, None,
            )
        });
        let frame = NrGraphFrame {
            color: handles[0],
            mask: handles[1],
            output: handles[2],
            destination: display.destination,
            guides,
            extent: resources.extent,
        };
        display.destination = frame.color;
        display.output_encoding = SdrOutputEncoding::Gamma22;
        Some(frame)
    }

    pub fn contribute<'a>(
        &'a self,
        graph: &mut RenderGraphBuilder<'a>,
        ctx: &'a SubsystemRenderCtx<'a>,
        frame: NrGraphFrame,
        snapshot: DlssFrameSnapshot,
        upstream: &'a Cell<DlssEvaluation>,
        evaluation: &'a RefCell<NrEvaluation>,
        dither: bool,
    ) {
        graph.add_pass(
            "dlss-nr",
            NrRgPass {
                session: self.session.as_ref().expect("NR session missing"),
                evaluation,
                upstream,
                upstream_required: snapshot.options.is_sr_active() || snapshot.options.is_rr_active(),
                color: frame.color,
                output: frame.output,
                mask: frame.mask,
                guides: frame.guides,
                reset: self.reset_pending,
                jitter: snapshot.constants.jitter_offset,
                intensity: self.effective.intensity,
            },
        );
        graph.add_pass(
            "nr-to-linear-sdr",
            NrDecodeRgPass {
                pass: self.decode.as_ref().unwrap(),
                evaluation,
                record_ctx: ctx.record_ctx,
                source: frame.output,
                destination: frame.destination,
                extent: frame.extent,
                dither,
            },
        );
        // 单套图像按提交顺序复用，导出状态覆盖本帧读/写与下一帧写入依赖。
        for image in [frame.color, frame.mask, frame.output] {
            graph.export_image(image, NrRgPass::WRITE, None);
        }
        // 引导图还可被 present graph 的 debug viewer 读取，恢复 Realtime 的既有跨图状态。
        let guide_state = if snapshot.options.is_sr_active() || snapshot.options.is_rr_active() {
            renderer_render_passes::post_process::dlss_sr::DLSS_SR_INPUT_READ
        } else {
            truvis_render_graph::render_graph::RgImageState::GENERAL
        };
        graph.export_image(frame.guides.depth, guide_state, None);
        graph.export_image(frame.guides.motion_vectors, guide_state, None);
    }

    /// 仅在实际 queue submit 成功后调用；GPU 完成由 Runtime timeline 另行确认。
    pub fn finish_submitted_frame(&mut self, evaluation: &NrEvaluation, frame_id: u64) {
        if matches!(evaluation, NrEvaluation::Succeeded) {
            if self.submitted_frames < 4 || self.reset_pending {
                log::info!(
                    "NR evaluate recorded/submitted frame={frame_id} reset={} intensity={}",
                    self.reset_pending,
                    self.effective.intensity
                );
            }
            self.submitted_frames += 1;
            self.reset_pending = false;
        }
    }
}

struct NrResources {
    extent: vk::Extent2D,
    images: [SingleImageTarget; 3],
}

impl NrResources {
    const CHANNELS: [(&'static str, vk::Format); 3] = [
        ("nr-color", vk::Format::R16G16B16A16_SFLOAT),
        ("nr-mask", vk::Format::R8G8B8A8_UNORM),
        ("nr-output", vk::Format::R16G16B16A16_SFLOAT),
    ];

    fn check_formats(ctx: &RenderRuntimeResourceCtx<'_>) -> Result<(), String> {
        let required = vk::FormatFeatureFlags::STORAGE_IMAGE
            | vk::FormatFeatureFlags::SAMPLED_IMAGE
            | vk::FormatFeatureFlags::TRANSFER_SRC
            | vk::FormatFeatureFlags::TRANSFER_DST;
        for (name, format) in Self::CHANNELS {
            if ctx.device_info_ctx.find_supported_format(&[format], vk::ImageTiling::OPTIMAL, required).is_empty() {
                return Err(format!("NR format unsupported: {name} {format:?}"));
            }
        }
        Ok(())
    }

    fn new(ctx: &mut RenderRuntimeResourceCtx<'_>) -> Self {
        let extent = ctx.frame_state.output_extent;
        let images = Self::CHANNELS.map(|(name, format)| {
            SingleImageTarget::new(
                ctx.resource_ctx,
                ctx.device_ctx,
                ctx.immediate_ctx,
                ctx.gfx_resource_registry,
                TargetImageDesc {
                    name_prefix: name,
                    format,
                    extent,
                    usage: vk::ImageUsageFlags::STORAGE
                        | vk::ImageUsageFlags::SAMPLED
                        | vk::ImageUsageFlags::TRANSFER_SRC
                        | vk::ImageUsageFlags::TRANSFER_DST,
                },
                ctx.frame_timing.frame_id(),
            )
        });
        let mask = ctx.gfx_resource_registry.get_image(images[1].image).unwrap().handle();
        ctx.immediate_ctx.one_time_exec(
            |cmd| {
                cmd.image_memory_barrier(
                    vk::DependencyFlags::empty(),
                    &[GfxImageBarrier::default()
                        .image(mask)
                        .src_mask(vk::PipelineStageFlags2::TOP_OF_PIPE, vk::AccessFlags2::empty())
                        .dst_mask(vk::PipelineStageFlags2::CLEAR, vk::AccessFlags2::TRANSFER_WRITE)
                        .layout_transfer(vk::ImageLayout::GENERAL, vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                        .image_aspect_flag(vk::ImageAspectFlags::COLOR)],
                );
                unsafe {
                    ctx.device_ctx.device().cmd_clear_color_image(
                        cmd.vk_handle(),
                        mask,
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        &vk::ClearColorValue { float32: [1.0; 4] },
                        &[vk::ImageSubresourceRange::default()
                            .aspect_mask(vk::ImageAspectFlags::COLOR)
                            .level_count(1)
                            .layer_count(1)],
                    );
                }
                cmd.image_memory_barrier(
                    vk::DependencyFlags::empty(),
                    &[GfxImageBarrier::default()
                        .image(mask)
                        .src_mask(vk::PipelineStageFlags2::CLEAR, vk::AccessFlags2::TRANSFER_WRITE)
                        .dst_mask(vk::PipelineStageFlags2::COMPUTE_SHADER, vk::AccessFlags2::SHADER_READ)
                        .layout_transfer(vk::ImageLayout::TRANSFER_DST_OPTIMAL, vk::ImageLayout::GENERAL)
                        .image_aspect_flag(vk::ImageAspectFlags::COLOR)],
                );
            },
            "nr-initialize-control-mask",
        );
        Self { extent, images }
    }
}
