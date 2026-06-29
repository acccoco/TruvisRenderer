use ash::vk;
use slotmap::Key;

use truvis_asset::handle::TextureBytes;
use truvis_gfx::gfx::{GfxDeviceCtx, GfxImmediateCtx, GfxResourceCtx};
use truvis_gfx::resources::image::GfxImage;
use truvis_gfx::resources::image_view::GfxImageViewDesc;
use truvis_gfx::resources::lifecycle::DestroyReason;
use truvis_gfx::resources::special_buffers::structured_buffer::GfxStructuredBuffer;
use truvis_render_foundation::handles::{GfxImageHandle, GfxImageViewHandle};
use truvis_shader_binding::gpu;
use truvis_world::SceneSkyState;
use truvis_world::guid_new_type::SceneTextureHandle;

use crate::bindings::bindless_manager::BindlessSrvHandle;
use crate::bindings::shader_binding_system::ShaderBindingSystem;
use crate::render_world::environment_binding::EnvironmentSkyBinding;
use crate::render_world::texture_resolver::TextureResolver;
use crate::resources::gfx_resource_manager::GfxResourceManager;

#[derive(Clone, Copy, Default)]
struct FallbackSkyTexture {
    image_handle: GfxImageHandle,
    view_handle: GfxImageViewHandle,
    srv_handle: BindlessSrvHandle,
}

/// 默认 sky 采样分布的 GPU 资源。
///
/// 该 buffer 通过 device address 写入 scene root buffer，shader 在 render 阶段只读访问。
/// 资源由 `RenderSkyManager` 拥有并在 shutdown 显式释放，不通过 bindless 表管理。
struct SkyDistributionResource {
    entries: GfxStructuredBuffer<gpu::scene::SkyDistributionEntry>,
    width: u32,
    height: u32,
    enabled: bool,
    version: u32,
}

struct SkyDistributionBuild {
    entries: Vec<gpu::scene::SkyDistributionEntry>,
    width: u32,
    height: u32,
}

impl SkyDistributionResource {
    fn new(
        resource_ctx: GfxResourceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        build: SkyDistributionBuild,
        version: u32,
        debug_name: impl AsRef<str>,
    ) -> Self {
        let entries = GfxStructuredBuffer::new_ssbo(resource_ctx, build.entries.len(), debug_name);
        entries.transfer_data_sync(resource_ctx, immediate_ctx, &build.entries);
        Self {
            entries,
            width: build.width,
            height: build.height,
            enabled: build.width > 0 && build.height > 0,
            version,
        }
    }

    fn destroy_mut(&mut self, resource_ctx: GfxResourceCtx<'_>) {
        self.entries.destroy_mut(resource_ctx, DestroyReason::Shutdown);
        self.enabled = false;
        self.width = 0;
        self.height = 0;
    }

    fn to_binding(&self) -> SkyDistributionBinding {
        SkyDistributionBinding {
            device_address: self.entries.device_address(),
            width: self.width,
            height: self.height,
            enabled: u32::from(self.enabled),
            version: self.version,
        }
    }
}

#[derive(Clone, Copy)]
struct SkyDistributionBinding {
    device_address: u64,
    width: u32,
    height: u32,
    enabled: u32,
    version: u32,
}

/// `RenderSkyManager::update_sky_binding` 的本帧解析结果。
pub(crate) struct SkyBindingUpdate {
    pub(crate) binding: EnvironmentSkyBinding,
    /// sky 绑定或 active distribution 是否变化；变化后当前 view temporal history 不再匹配。
    pub(crate) changed: bool,
}

/// scene sky 的 runtime 私有桥接层。
///
/// `SceneStore` 只负责 sky 语义状态；真实 GPU texture 由 `RenderTextureManager`
/// 管理。`RenderSkyManager` 缓存本帧 sky state 并持有一个常驻纯色 fallback，保证 shader 侧
/// scene root buffer 始终写入合法 SRV 和与之匹配的 sky importance distribution。
pub(crate) struct RenderSkyManager {
    sky_texture: Option<SceneTextureHandle>,
    sky_enabled: bool,
    sky_intensity: f32,
    sky_revision: u64,
    fallback: FallbackSkyTexture,
    fallback_distribution: SkyDistributionResource,
    sky_distribution: Option<SkyDistributionResource>,
    retired_distributions: Vec<SkyDistributionResource>,
    next_distribution_version: u32,
    last_active_distribution_version: u32,
    using_real_sky: bool,
}

impl RenderSkyManager {
    /// 创建立即可用的纯色 fallback sky。
    pub(crate) fn new(
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        gfx_resource_manager: &mut GfxResourceManager,
        shader_binding_system: &mut ShaderBindingSystem,
    ) -> Self {
        let _span = tracy_client::span!("RenderSkyManager::new");
        let fallback = Self::create_fallback_sky(
            resource_ctx,
            device_ctx,
            immediate_ctx,
            gfx_resource_manager,
            shader_binding_system,
        );
        let fallback_distribution = SkyDistributionResource::new(
            resource_ctx,
            immediate_ctx,
            Self::build_fallback_distribution(),
            1,
            "FallbackSkyDistribution",
        );

        Self {
            sky_texture: None,
            sky_enabled: true,
            sky_intensity: 1.0,
            sky_revision: 0,
            fallback,
            fallback_distribution,
            sky_distribution: None,
            retired_distributions: Vec::new(),
            next_distribution_version: 2,
            last_active_distribution_version: 1,
            using_real_sky: false,
        }
    }

    /// 同步 CPU scene 中的 sky 语义状态。
    ///
    /// `SceneStore` 是 enabled/intensity/texture 的权威 owner；这里仅缓存当前 prepare
    /// 已应用的值，用于选择本帧 sky binding 和决定旧 distribution 是否需要退休。
    pub(crate) fn apply_scene_sky_state(&mut self, state: &SceneSkyState) -> bool {
        let state_changed = self.sky_revision != state.revision;
        if self.sky_texture != state.texture {
            if let Some(old_distribution) = self.sky_distribution.take() {
                // distribution device address 可能仍被上一轮 scene root buffer 引用，延迟到
                // shutdown 统一释放，保持当前 RenderSkyManager 的资源生命周期策略。
                self.retired_distributions.push(old_distribution);
            }
            self.sky_texture = state.texture;
        }
        self.sky_enabled = state.enabled;
        self.sky_intensity = state.intensity;
        self.sky_revision = state.revision;
        state_changed
    }

    /// 观察当前 scene sky 的 CPU texture bytes，并在 texture manager 上传真实 sky image 前构建采样分布。
    ///
    /// `World::sync_for_render` 的 texture upload payload 只会被消费一次，因此分布构建必须发生在
    /// `RenderWorld::prepare_asset_sync`。真实 sky image 仍由 `RenderTextureManager` 上传；本 bridge 只拥有与默认 sky
    /// 采样语义绑定的 alias table。
    pub(crate) fn observe_texture_loaded(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        handle: SceneTextureHandle,
        data: &TextureBytes,
    ) {
        if Some(handle) != self.sky_texture {
            return;
        }

        let Some(build) = Self::build_distribution_from_texture(data) else {
            log::warn!(
                "RenderSkyManager: default sky distribution fell back to uniform because texture data is invalid"
            );
            return;
        };

        if let Some(old_distribution) = self.sky_distribution.take() {
            // scene root buffer 通过 device address 引用 sky distribution；旧表可能仍被在飞帧读取，
            // 因此不在重建当帧立即释放，统一留到 shutdown 阶段销毁。
            self.retired_distributions.push(old_distribution);
        }

        let version = self.next_distribution_version;
        self.next_distribution_version = self.next_distribution_version.saturating_add(1).max(2);
        self.sky_distribution =
            Some(SkyDistributionResource::new(resource_ctx, immediate_ctx, build, version, "DefaultSkyDistribution"));
        log::info!("RenderSkyManager: scene sky importance distribution is ready, version={version}");
    }

    pub(crate) fn observe_texture_failed(&mut self, handle: SceneTextureHandle, error: &str) {
        if Some(handle) == self.sky_texture {
            log::warn!("RenderSkyManager: scene sky texture failed; keep fallback sky distribution: {error}");
        }
    }

    /// 解析本帧 sky 绑定；真实 sky 未 GPU ready 时返回纯色 fallback。
    ///
    /// 这里故意先检查 `is_texture_ready`，避免 `TextureResolver::resolve_texture` 在未就绪时
    /// 返回材质专用的洋红 fallback。sky 的降级策略由本 bridge 独立定义。
    pub(crate) fn update_sky_binding(
        &mut self,
        scene_sky: &SceneSkyState,
        texture_resolver: &dyn TextureResolver,
    ) -> SkyBindingUpdate {
        let scene_changed = self.apply_scene_sky_state(scene_sky);
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

        let mut distribution = self.active_distribution(real_ready).to_binding();
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
                sampler: gpu::bindless::ESamplerType_LinearClamp,
                distribution_device_address: distribution.device_address,
                distribution_width: distribution.width,
                distribution_height: distribution.height,
                distribution_enabled: distribution.enabled,
                distribution_version: distribution.version,
            }
        } else {
            self.fallback_binding(distribution)
        };

        SkyBindingUpdate {
            binding,
            changed: scene_changed || sky_source_changed || distribution_changed,
        }
    }

    /// 释放 fallback sky。真实 sky texture 由 `RenderTextureManager` 释放。
    pub(crate) fn destroy_mut(
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
        if let Some(mut distribution) = self.sky_distribution.take() {
            distribution.destroy_mut(resource_ctx);
        }
        for distribution in &mut self.retired_distributions {
            distribution.destroy_mut(resource_ctx);
        }
        self.retired_distributions.clear();
        self.fallback_distribution.destroy_mut(resource_ctx);
    }

    fn active_distribution(&self, real_ready: bool) -> &SkyDistributionResource {
        if real_ready {
            if let Some(distribution) = &self.sky_distribution {
                return distribution;
            }
        }
        &self.fallback_distribution
    }

    fn fallback_binding(&self, distribution: SkyDistributionBinding) -> EnvironmentSkyBinding {
        EnvironmentSkyBinding {
            srv_handle: self.fallback.srv_handle,
            sampler: gpu::bindless::ESamplerType_LinearClamp,
            distribution_device_address: distribution.device_address,
            distribution_width: distribution.width,
            distribution_height: distribution.height,
            distribution_enabled: distribution.enabled,
            distribution_version: distribution.version,
        }
    }

    fn build_fallback_distribution() -> SkyDistributionBuild {
        SkyDistributionBuild {
            entries: vec![gpu::scene::SkyDistributionEntry {
                alias_probability: 1.0,
                solid_angle_pdf: 1.0 / (4.0 * std::f32::consts::PI),
                alias_index: 0,
                _padding_0: 0,
            }],
            width: 1,
            height: 1,
        }
    }

    fn build_distribution_from_texture(data: &TextureBytes) -> Option<SkyDistributionBuild> {
        let width = data.extent.width;
        let height = data.extent.height;
        if width == 0 || height == 0 {
            return None;
        }

        let texel_count = width.checked_mul(height)? as usize;
        if data.pixels.len() < texel_count.checked_mul(4)? {
            return None;
        }

        let mut weights = vec![0.0_f64; texel_count];
        let mut solid_angles = vec![0.0_f64; texel_count];
        let mut total_weight = 0.0_f64;
        for y in 0..height {
            let solid_angle = Self::lat_long_texel_solid_angle(y, width, height);
            for x in 0..width {
                let idx = (y * width + x) as usize;
                let pixel = &data.pixels[idx * 4..idx * 4 + 4];
                let luminance = Self::rgba8_luminance(pixel);
                let weight = luminance * solid_angle;
                weights[idx] = weight;
                solid_angles[idx] = solid_angle;
                total_weight += weight;
            }
        }

        if total_weight <= f64::EPSILON {
            return None;
        }

        let (alias_probability, alias_index) = Self::build_alias_table(&weights, total_weight)?;
        let entries = weights
            .iter()
            .zip(solid_angles.iter())
            .zip(alias_probability.iter().zip(alias_index.iter()))
            .map(|((weight, solid_angle), (probability, alias))| {
                let texel_probability = *weight / total_weight;
                let solid_angle_pdf = if *solid_angle > 0.0 { (texel_probability / *solid_angle) as f32 } else { 0.0 };
                gpu::scene::SkyDistributionEntry {
                    alias_probability: *probability,
                    solid_angle_pdf,
                    alias_index: *alias,
                    _padding_0: 0,
                }
            })
            .collect();

        Some(SkyDistributionBuild { entries, width, height })
    }

    fn lat_long_texel_solid_angle(row: u32, width: u32, height: u32) -> f64 {
        let dphi = 2.0 * std::f64::consts::PI / f64::from(width);
        let v0 = f64::from(row) / f64::from(height);
        let v1 = f64::from(row + 1) / f64::from(height);
        let theta_top = (0.5 - v0) * std::f64::consts::PI;
        let theta_bottom = (0.5 - v1) * std::f64::consts::PI;
        dphi * (theta_top.sin() - theta_bottom.sin()).max(0.0)
    }

    fn rgba8_luminance(pixel: &[u8]) -> f64 {
        let r = f64::from(pixel[0]) / 255.0;
        let g = f64::from(pixel[1]) / 255.0;
        let b = f64::from(pixel[2]) / 255.0;
        0.2126 * r + 0.7152 * g + 0.0722 * b
    }

    fn build_alias_table(weights: &[f64], total_weight: f64) -> Option<(Vec<f32>, Vec<u32>)> {
        let count = weights.len();
        if count == 0 || total_weight <= f64::EPSILON || count > u32::MAX as usize {
            return None;
        }

        let mut scaled: Vec<f64> = weights.iter().map(|weight| weight * count as f64 / total_weight).collect();
        let mut small = Vec::new();
        let mut large = Vec::new();
        for (idx, probability) in scaled.iter().enumerate() {
            if *probability < 1.0 {
                small.push(idx);
            } else {
                large.push(idx);
            }
        }

        let mut alias_probability = vec![1.0_f32; count];
        let mut alias_index: Vec<u32> = (0..count as u32).collect();
        while !small.is_empty() && !large.is_empty() {
            let small_idx = small.pop().unwrap();
            let large_idx = large.pop().unwrap();
            alias_probability[small_idx] = scaled[small_idx].clamp(0.0, 1.0) as f32;
            alias_index[small_idx] = large_idx as u32;

            scaled[large_idx] = (scaled[large_idx] + scaled[small_idx]) - 1.0;
            if scaled[large_idx] < 1.0 {
                small.push(large_idx);
            } else {
                large.push(large_idx);
            }
        }

        for idx in small.into_iter().chain(large.into_iter()) {
            alias_probability[idx] = 1.0;
            alias_index[idx] = idx as u32;
        }

        Some((alias_probability, alias_index))
    }

    fn create_fallback_sky(
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        gfx_resource_manager: &mut GfxResourceManager,
        shader_binding_system: &mut ShaderBindingSystem,
    ) -> FallbackSkyTexture {
        // sky fallback 需要视觉中性，避免材质缺失用的洋红色污染环境光。
        // shader 当前会将 sky sample 乘以 8，因此这里保持低亮度灰蓝。
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
        debug_assert!(self.fallback.image_handle.is_null());
        debug_assert!(self.fallback.view_handle.is_null());
        debug_assert!(self.sky_distribution.is_none());
        debug_assert!(self.retired_distributions.is_empty());
    }
}
