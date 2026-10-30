//! Realtime 渲染子系统持有的 GBuffer 资源管理。
//!
//! GBuffer 由三张 per-FIF storage 纹理组成，记录 RT 首次命中的几何与材质信息，
//! 供降噪等后处理 pass 读取。通道布局与 shader 侧 `GBufferData`（`gbuffer.slangi`）对应：
//!
//! | 通道 | 格式 | 内容 |
//! |------|------|------|
//! | A | R16G16B16A16_SFLOAT | world-space forward/shading normal.xyz + roughness |
//! | B | R16G16B16A16_SFLOAT | world_position.xyz + linear_depth |
//! | C | R8G8B8A8_UNORM | albedo.rgb + metallic |
//!
//! 生命周期由 `RealtimeRenderSubsystem` 管理：init 创建、on_resize 重建、shutdown 销毁。
//! engine 层不再持有 GBuffer 资源。

use ash::vk;

use truvis_gfx::gfx::{GfxDeviceCtx, GfxImmediateCtx, GfxResourceCtx};
use truvis_gfx::resources::lifecycle::DestroyReason;
use truvis_render_foundation::frame_label::FrameLabel;
use truvis_render_foundation::handles::{GfxImageHandle, GfxImageViewHandle};
use truvis_render_runtime::resources::gfx_resource_registry::GfxResourceRegistry;

use crate::shared::targets::{PerFrameImageSet, TargetImageDesc};

/// Realtime 渲染子系统使用的 GBuffer 资源集合。
///
/// 持有三个通道（A/B/C）的 per-FIF storage image 和对应 image view。RT raygen pass 通过
/// pass-local descriptor 写入；调试 UI 只在显示当前图像时通过自己的 descriptor 采样。
///
/// 格式和通道语义是渲染策略决策，由 Renderer 决定，不属于 engine 基础设施。
pub struct GBuffer {
    /// GBufferA：world-space forward/shading normal.xyz + 粗糙度 roughness (R16G16B16A16_SFLOAT)
    a: PerFrameImageSet,
    /// GBufferB：世界位置 world_position.xyz + 线性深度 linear_depth (R16G16B16A16_SFLOAT)
    b: PerFrameImageSet,
    /// GBufferC：反照率 albedo.rgb + 金属度 metallic (R8G8B8A8_UNORM)
    c: PerFrameImageSet,
    extent: vk::Extent2D,
}

impl GBuffer {
    pub fn new(
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        gfx_resource_registry: &mut GfxResourceRegistry,
        extent: vk::Extent2D,
        frame_id: u64,
    ) -> Self {
        let a = PerFrameImageSet::new(
            resource_ctx,
            device_ctx,
            immediate_ctx,
            gfx_resource_registry,
            TargetImageDesc {
                name_prefix: "gbuffer-a",
                format: Self::A_FORMAT,
                extent,
                usage: vk::ImageUsageFlags::STORAGE | vk::ImageUsageFlags::SAMPLED,
            },
            frame_id,
        );
        let b = PerFrameImageSet::new(
            resource_ctx,
            device_ctx,
            immediate_ctx,
            gfx_resource_registry,
            TargetImageDesc {
                name_prefix: "gbuffer-b",
                format: Self::B_FORMAT,
                extent,
                usage: vk::ImageUsageFlags::STORAGE | vk::ImageUsageFlags::SAMPLED,
            },
            frame_id,
        );
        let c = PerFrameImageSet::new(
            resource_ctx,
            device_ctx,
            immediate_ctx,
            gfx_resource_registry,
            TargetImageDesc {
                name_prefix: "gbuffer-c",
                format: Self::C_FORMAT,
                extent,
                usage: vk::ImageUsageFlags::STORAGE | vk::ImageUsageFlags::SAMPLED,
            },
            frame_id,
        );

        Self {
            a,
            b,
            c,
            extent,
        }
    }

    /// 窗口尺寸变化时重建所有 GBuffer 资源。
    pub fn rebuild(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        gfx_resource_registry: &mut GfxResourceRegistry,
        extent: vk::Extent2D,
        frame_id: u64,
    ) {
        self.destroy(resource_ctx, device_ctx, gfx_resource_registry, DestroyReason::Resize);
        *self = Self::new(resource_ctx, device_ctx, immediate_ctx, gfx_resource_registry, extent, frame_id);
    }

    /// 释放所有 GBuffer GPU 资源。
    pub fn destroy(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        gfx_resource_registry: &mut GfxResourceRegistry,
        reason: DestroyReason,
    ) {
        PerFrameImageSet::destroy_all(
            &mut [&mut self.a, &mut self.b, &mut self.c],
            resource_ctx,
            device_ctx,
            gfx_resource_registry,
            reason,
        );
    }
}

// 访问器与格式常量
impl GBuffer {
    pub const A_FORMAT: vk::Format = vk::Format::R16G16B16A16_SFLOAT;
    pub const B_FORMAT: vk::Format = vk::Format::R16G16B16A16_SFLOAT;
    pub const C_FORMAT: vk::Format = vk::Format::R8G8B8A8_UNORM;

    /// GBufferA（forward/shading 法线 + 粗糙度）的当前帧 image 和 view handle。
    #[inline]
    pub fn a_handle(&self, frame_label: FrameLabel) -> (GfxImageHandle, GfxImageViewHandle) {
        let target = self.a.target(frame_label);
        (target.image, target.view)
    }

    /// GBufferB（世界位置 + 线性深度）的当前帧 image 和 view handle。
    #[inline]
    pub fn b_handle(&self, frame_label: FrameLabel) -> (GfxImageHandle, GfxImageViewHandle) {
        let target = self.b.target(frame_label);
        (target.image, target.view)
    }

    /// GBufferC（反照率 + 金属度）的当前帧 image 和 view handle。
    #[inline]
    pub fn c_handle(&self, frame_label: FrameLabel) -> (GfxImageHandle, GfxImageViewHandle) {
        let target = self.c.target(frame_label);
        (target.image, target.view)
    }

    #[inline]
    pub fn extent(&self) -> vk::Extent2D {
        self.extent
    }
}
