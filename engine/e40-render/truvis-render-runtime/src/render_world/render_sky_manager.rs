use ash::vk;
use slotmap::Key;

use truvis_asset::handle::TextureBytes;
use truvis_gfx::gfx::{GfxDeviceCtx, GfxImmediateCtx, GfxQueueCtx, GfxResourceCtx};
use truvis_gfx::resources::image::GfxImage;
use truvis_gfx::resources::image_view::GfxImageViewDesc;
use truvis_gfx::resources::lifecycle::DestroyReason;
use truvis_gfx::resources::special_buffers::structured_buffer::GfxStructuredBuffer;
use truvis_render_foundation::frame_counter::{FrameCounter, FrameToken};
use truvis_render_foundation::handles::{GfxBufferHandle, GfxImageHandle, GfxImageViewHandle};
use truvis_shader_binding::gpu;
use truvis_world::SceneSkyState;
use truvis_world::guid_new_type::TextureHandle;

use crate::bindings::bindless_manager::BindlessSrvHandle;
use crate::bindings::shader_binding_system::ShaderBindingSystem;
use crate::render_world::environment_binding::EnvironmentSkyBinding;
use crate::render_world::render_asset_upload_queue::{CompletedSkyDistributionUpload, RenderAssetUploadQueue};
use crate::render_world::sky_distribution_builder::{
    SkyDistributionBuildRequest, SkyDistributionBuildResult, SkyDistributionBuilder,
};
use crate::render_world::texture_resolver::TextureResolver;
use crate::resources::gfx_resource_manager::GfxResourceManager;

#[derive(Clone, Copy, Default)]
struct FallbackSkyTexture {
    image_handle: GfxImageHandle,
    view_handle: GfxImageViewHandle,
    srv_handle: BindlessSrvHandle,
}

/// 1x1 uniform sphere distribution。
///
/// fallback 体积极小且整个 runtime 常驻，继续使用同步创建；大型真实 HDRI distribution
/// 则必须通过 worker + shared transfer timeline 异步发布。
struct FallbackSkyDistribution {
    entries: GfxStructuredBuffer<gpu::engine::scene::SkyDistributionEntry>,
    version: u32,
}

impl FallbackSkyDistribution {
    fn new(resource_ctx: GfxResourceCtx<'_>, immediate_ctx: GfxImmediateCtx<'_>) -> Self {
        let entry = gpu::engine::scene::SkyDistributionEntry {
            alias_probability: 1.0,
            solid_angle_pdf: 1.0 / (4.0 * std::f32::consts::PI),
            alias_index: 0,
            _padding_0: 0,
        };
        let entries = GfxStructuredBuffer::new_ssbo(resource_ctx, 1, "FallbackSkyDistribution");
        entries.transfer_data_sync(resource_ctx, immediate_ctx, std::slice::from_ref(&entry));
        Self { entries, version: 1 }
    }

    fn to_binding(&self) -> SkyDistributionBinding {
        SkyDistributionBinding {
            device_address: self.entries.device_address(),
            width: 1,
            height: 1,
            enabled: 1,
            version: self.version,
        }
    }

    fn destroy_mut(&mut self, resource_ctx: GfxResourceCtx<'_>) {
        self.entries.destroy_mut(resource_ctx, DestroyReason::Shutdown);
    }
}

/// 已发布、可由 scene root device address 引用的真实天空 Alias buffer。
struct SkyDistributionResource {
    buffer_handle: GfxBufferHandle,
    device_address: u64,
    width: u32,
    height: u32,
    version: u32,
}

impl SkyDistributionResource {
    fn to_binding(&self) -> SkyDistributionBinding {
        SkyDistributionBinding {
            device_address: self.device_address,
            width: self.width,
            height: self.height,
            enabled: 1,
            version: self.version,
        }
    }
}

/// 已交给 `GfxResourceManager` 按 FIF 延迟释放的 distribution。
///
/// 本地记录只用于在 `RenderWorld::begin_frame` 后收敛 manager 状态；真实资源 owner
/// 是 `GfxResourceManager::pending_destroy_buffers`。
struct RetiredSkyDistribution {
    buffer_handle: GfxBufferHandle,
    retired_frame_id: u64,
}

#[derive(Clone, Copy)]
struct SkyDistributionBinding {
    device_address: u64,
    width: u32,
    height: u32,
    enabled: u32,
    version: u32,
}

/// sky 阶段对 dirty routing 暴露的结构化结果。
pub(crate) struct RenderSkyUpdateResult {
    pub(crate) binding: EnvironmentSkyBinding,
    /// sky 绑定或 active distribution 是否变化；变化后当前 view temporal history 不再匹配。
    pub(crate) changed: bool,
}

/// scene sky 的 runtime 私有桥接层。
///
/// `SceneStore` 只保存 `TextureHandle` 与天空语义；真实 image 由 `RenderTextureManager`
/// 持有。该 manager 拥有 distribution worker、请求 generation、active/retired Alias
/// buffer，并保证新 texture 绝不引用旧 texture 的 distribution。
pub(crate) struct RenderSkyManager {
    sky_texture: Option<TextureHandle>,
    sky_enabled: bool,
    sky_intensity: f32,
    sky_revision: u64,
    fallback: FallbackSkyTexture,
    fallback_distribution: FallbackSkyDistribution,
    sky_distribution: Option<SkyDistributionResource>,
    retired_distributions: Vec<RetiredSkyDistribution>,
    distribution_builder: SkyDistributionBuilder,
    next_request_id: u64,
    latest_request: Option<(u64, TextureHandle)>,
    next_distribution_version: u32,
    last_active_distribution_version: u32,
    current_frame_id: u64,
    using_real_sky: bool,
    state_changed_pending: bool,
    worker_stopped: bool,
}

impl RenderSkyManager {
    /// 创建立即可用的纯色 fallback sky 与 uniform sphere distribution。
    pub(crate) fn new(
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        gfx_resource_manager: &mut GfxResourceManager,
        shader_binding_system: &mut ShaderBindingSystem,
        frame_token: FrameToken,
    ) -> Self {
        let _span = tracy_client::span!("RenderSkyManager::new");
        let fallback = Self::create_fallback_sky(
            resource_ctx,
            device_ctx,
            immediate_ctx,
            gfx_resource_manager,
            shader_binding_system,
        );
        let fallback_distribution = FallbackSkyDistribution::new(resource_ctx, immediate_ctx);

        Self {
            sky_texture: None,
            sky_enabled: true,
            sky_intensity: 1.0,
            sky_revision: 0,
            fallback,
            fallback_distribution,
            sky_distribution: None,
            retired_distributions: Vec::new(),
            distribution_builder: SkyDistributionBuilder::new(),
            next_request_id: 1,
            latest_request: None,
            next_distribution_version: 2,
            last_active_distribution_version: 1,
            current_frame_id: frame_token.frame_id(),
            using_real_sky: false,
            state_changed_pending: false,
            worker_stopped: false,
        }
    }

    /// 在 RenderRuntime 已等待当前 FIF slot 后推进 frame id，并收敛 retired bookkeeping。
    pub(crate) fn begin_frame(&mut self, frame_token: FrameToken) {
        self.current_frame_id = frame_token.frame_id();
        let fif = FrameCounter::fif_count() as u64;
        self.retired_distributions
            .retain(|retired| retired.retired_frame_id.saturating_add(fif) > self.current_frame_id);
    }

    /// 同步 CPU scene 中的 sky 语义状态。
    pub(crate) fn apply_scene_sky_state(
        &mut self,
        state: &SceneSkyState,
        gfx_resource_manager: &mut GfxResourceManager,
    ) -> bool {
        let state_changed = self.sky_revision != state.revision;
        if self.sky_texture != state.texture {
            // texture identity 一改变就先撤下旧 distribution。新 image ready 而新 Alias
            // 尚未完成时只能配 uniform PDF，不能短暂复用旧表。
            self.retire_active_distribution(gfx_resource_manager);
            self.latest_request = None;
            self.sky_texture = state.texture;
        }
        self.sky_enabled = state.enabled;
        self.sky_intensity = state.intensity;
        self.sky_revision = state.revision;
        self.state_changed_pending |= state_changed;
        state_changed
    }

    /// 共享 texture payload 给单线程 distribution builder，不阻塞渲染线程。
    pub(crate) fn observe_texture_loaded(
        &mut self,
        handle: TextureHandle,
        data: &TextureBytes,
        gfx_resource_manager: &mut GfxResourceManager,
    ) {
        if Some(handle) != self.sky_texture {
            return;
        }

        // 同一 handle 的 reload 也可能代表不同像素；在新请求开始时撤下旧表，保持
        // image/distribution generation 一致。
        self.retire_active_distribution(gfx_resource_manager);
        let request_id = self.next_request_id;
        self.next_request_id = self.next_request_id.checked_add(1).expect("RenderSkyManager request id exhausted");
        self.latest_request = Some((request_id, handle));
        self.distribution_builder.request(SkyDistributionBuildRequest {
            request_id,
            texture: handle,
            texture_bytes: data.clone(),
        });
    }

    pub(crate) fn observe_texture_failed(
        &mut self,
        handle: TextureHandle,
        error: &str,
        gfx_resource_manager: &mut GfxResourceManager,
    ) {
        if Some(handle) == self.sky_texture {
            self.latest_request = None;
            self.retire_active_distribution(gfx_resource_manager);
            self.state_changed_pending = true;
            log::warn!("RenderSkyManager: scene sky texture failed; keep fallback sky distribution: {error}");
        }
    }

    /// 收集 CPU worker 结果，并把仍为最新 generation 的 Alias entries 提交到共享 transfer queue。
    pub(crate) fn submit_completed_builds(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        queue_ctx: GfxQueueCtx<'_>,
        upload_queue: &mut RenderAssetUploadQueue,
    ) {
        for result in self.distribution_builder.poll() {
            match result {
                SkyDistributionBuildResult::Ready(build) => {
                    if !self.is_latest_request(build.request_id, build.texture) {
                        log::debug!(
                            "RenderSkyManager: discard stale CPU distribution request={} texture={:?}",
                            build.request_id,
                            build.texture
                        );
                        continue;
                    }
                    if let Err(error) = upload_queue.submit_sky_distribution(resource_ctx, device_ctx, queue_ctx, build)
                    {
                        log::error!("RenderSkyManager: failed to submit sky distribution upload: {error}");
                    }
                }
                SkyDistributionBuildResult::UniformFallback {
                    request_id,
                    texture,
                    source_width,
                    source_height,
                    cpu_build_elapsed,
                } => {
                    if self.is_latest_request(request_id, texture) {
                        self.state_changed_pending = true;
                        log::warn!(
                            "RenderSkyManager: sky {:?} {}x{} has zero/invalid energy; use uniform sphere PDF (CPU build {:.2} ms)",
                            texture,
                            source_width,
                            source_height,
                            cpu_build_elapsed.as_secs_f64() * 1000.0
                        );
                    }
                }
            }
        }
    }

    /// 发布 timeline 已完成且 generation 仍匹配的 distribution buffer。
    pub(crate) fn publish_completed_uploads(
        &mut self,
        completed_uploads: Vec<CompletedSkyDistributionUpload>,
        resource_ctx: GfxResourceCtx<'_>,
        gfx_resource_manager: &mut GfxResourceManager,
    ) {
        for completed in completed_uploads {
            if !self.is_latest_request(completed.request_id, completed.texture) {
                log::debug!(
                    "RenderSkyManager: destroy stale GPU distribution request={} texture={:?}",
                    completed.request_id,
                    completed.texture
                );
                completed.buffer.destroy(resource_ctx, DestroyReason::DeferredCleanup);
                continue;
            }

            self.retire_active_distribution(gfx_resource_manager);
            let device_address = completed.buffer.device_address();
            let buffer_handle = gfx_resource_manager.register_buffer(completed.buffer);
            let version = self.next_distribution_version;
            self.next_distribution_version = self.next_distribution_version.saturating_add(1).max(2);
            self.sky_distribution = Some(SkyDistributionResource {
                buffer_handle,
                device_address,
                width: completed.width,
                height: completed.height,
                version,
            });
            self.state_changed_pending = true;

            let entry_count = completed.width as u64 * completed.height as u64;
            let gpu_bytes = entry_count * std::mem::size_of::<gpu::engine::scene::SkyDistributionEntry>() as u64;
            log::info!(
                "RenderSkyManager: published sky distribution request={} source={}x{} distribution={}x{} entries={} GPU={} bytes CPU={:.2} ms upload={:.2} ms version={}",
                completed.request_id,
                completed.source_width,
                completed.source_height,
                completed.width,
                completed.height,
                entry_count,
                gpu_bytes,
                completed.cpu_build_elapsed.as_secs_f64() * 1000.0,
                completed.upload_elapsed.as_secs_f64() * 1000.0,
                version
            );
        }
    }

    /// 解析本帧 sky 绑定；真实 image 未 ready 时显示纯色 fallback。
    ///
    /// 真实 image ready 但 Alias 尚未发布时，仍绑定真实 HDRI，并用 1x1 uniform sphere
    /// distribution。这样 CPU/GPU 异步过程不会制造黑帧，也不会把旧 Alias 配给新图。
    pub(crate) fn update_sky_binding(&mut self, texture_resolver: &dyn TextureResolver) -> RenderSkyUpdateResult {
        let scene_changed = std::mem::take(&mut self.state_changed_pending);
        let real_ready =
            self.sky_texture.is_some_and(|texture| self.sky_enabled && texture_resolver.is_texture_ready(texture));
        let sky_source_changed = self.using_real_sky != real_ready;
        self.using_real_sky = real_ready;

        if sky_source_changed {
            if real_ready {
                log::info!("RenderSkyManager: scene sky is GPU ready; switch from fallback sky");
            } else {
                log::warn!("RenderSkyManager: scene sky is not GPU ready; switch to fallback sky");
            }
        }

        let mut distribution = self.active_distribution_binding(real_ready);
        if !self.sky_enabled {
            distribution.enabled = 0;
        }
        let distribution_changed = self.last_active_distribution_version != distribution.version;
        self.last_active_distribution_version = distribution.version;

        let binding = if real_ready {
            let texture = texture_resolver.resolve_texture(
                self.sky_texture.expect("RenderSkyManager: real_ready requires a scene texture handle"),
            );
            EnvironmentSkyBinding {
                srv_handle: texture.srv_handle,
                sampler: gpu::engine::bindless::ESamplerType_LinearRepeatClamp,
                distribution_device_address: distribution.device_address,
                distribution_width: distribution.width,
                distribution_height: distribution.height,
                distribution_enabled: distribution.enabled,
                distribution_version: distribution.version,
            }
        } else {
            self.fallback_binding(distribution)
        };

        RenderSkyUpdateResult {
            binding,
            changed: scene_changed || sky_source_changed || distribution_changed,
        }
    }

    /// shutdown 第一步：停止并 join CPU producer。
    pub(crate) fn stop_worker(&mut self) {
        if self.worker_stopped {
            return;
        }
        self.distribution_builder.shutdown();
        self.worker_stopped = true;
    }

    /// shutdown 最后阶段：销毁已发布、retired 与 fallback 资源。
    ///
    /// 调用前 shared transfer queue 已完成等待并销毁 pending distribution，因此此处
    /// 不会与 transfer queue 争用 buffer。
    pub(crate) fn destroy_gpu_resources(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        shader_binding_system: &mut ShaderBindingSystem,
        gfx_resource_manager: &mut GfxResourceManager,
    ) {
        if !self.fallback.view_handle.is_null() {
            shader_binding_system.unregister_srv(self.fallback.view_handle);
        }
        if !self.fallback.image_handle.is_null() {
            gfx_resource_manager.release_image_immediate(
                resource_ctx,
                device_ctx,
                self.fallback.image_handle,
                DestroyReason::Shutdown,
            );
        }
        self.fallback = FallbackSkyTexture::default();

        if let Some(distribution) = self.sky_distribution.take() {
            gfx_resource_manager.release_buffer_immediate(
                resource_ctx,
                distribution.buffer_handle,
                DestroyReason::Shutdown,
            );
        }
        for retired in self.retired_distributions.drain(..) {
            gfx_resource_manager.release_buffer_immediate(resource_ctx, retired.buffer_handle, DestroyReason::Shutdown);
        }
        self.fallback_distribution.destroy_mut(resource_ctx);
    }

    fn is_latest_request(&self, request_id: u64, texture: TextureHandle) -> bool {
        self.latest_request == Some((request_id, texture)) && self.sky_texture == Some(texture)
    }

    fn retire_active_distribution(&mut self, gfx_resource_manager: &mut GfxResourceManager) {
        let Some(distribution) = self.sky_distribution.take() else {
            return;
        };
        gfx_resource_manager.release_buffer_deferred(distribution.buffer_handle, self.current_frame_id);
        self.retired_distributions.push(RetiredSkyDistribution {
            buffer_handle: distribution.buffer_handle,
            retired_frame_id: self.current_frame_id,
        });
    }

    fn active_distribution_binding(&self, real_ready: bool) -> SkyDistributionBinding {
        if real_ready {
            if let Some(distribution) = &self.sky_distribution {
                return distribution.to_binding();
            }
        }
        self.fallback_distribution.to_binding()
    }

    fn fallback_binding(&self, distribution: SkyDistributionBinding) -> EnvironmentSkyBinding {
        EnvironmentSkyBinding {
            srv_handle: self.fallback.srv_handle,
            sampler: gpu::engine::bindless::ESamplerType_LinearRepeatClamp,
            distribution_device_address: distribution.device_address,
            distribution_width: distribution.width,
            distribution_height: distribution.height,
            distribution_enabled: distribution.enabled,
            distribution_version: distribution.version,
        }
    }

    fn create_fallback_sky(
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        gfx_resource_manager: &mut GfxResourceManager,
        shader_binding_system: &mut ShaderBindingSystem,
    ) -> FallbackSkyTexture {
        // sky fallback 需要视觉中性，避免材质缺失用的洋红色污染环境光。
        // PathTracingCommonSettings::sky_brightness 仍是唯一生效的天空亮度控制。
        let pixels: [u8; 4] = [10, 13, 15, 255];
        let image = GfxImage::from_rgba8(resource_ctx, immediate_ctx, 1, 1, &pixels, "FallbackSky");
        let image_format = image.format();

        let image_handle = gfx_resource_manager.register_image(image);
        let view_handle = gfx_resource_manager.get_or_create_image_view(
            device_ctx,
            image_handle,
            GfxImageViewDesc::new_2d(image_format, vk::ImageAspectFlags::COLOR),
            "FallbackSkyView",
        );
        shader_binding_system.register_srv(view_handle);
        let srv_handle = shader_binding_system.get_shader_srv_handle(view_handle);

        FallbackSkyTexture {
            image_handle,
            view_handle,
            srv_handle,
        }
    }
}

impl Drop for RenderSkyManager {
    fn drop(&mut self) {
        debug_assert!(self.worker_stopped);
        debug_assert!(self.fallback.image_handle.is_null());
        debug_assert!(self.fallback.view_handle.is_null());
        debug_assert!(self.sky_distribution.is_none());
        debug_assert!(self.retired_distributions.is_empty());
    }
}
