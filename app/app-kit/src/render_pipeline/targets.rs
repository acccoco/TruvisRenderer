//! App 层窗口尺寸渲染目标。
//!
//! 这些资源描述具体管线需要的中间图像，而不是 engine 的帧调度基础设施。
//! owner 只保存 `GfxResourceManager` handle；创建、resize 和 shutdown 时由
//! `RtPipeline` 通过生命周期 ctx 显式传入 manager 与 typed Gfx ctx。
//!
//! 设计边界：
//! - `FrameCounter` / `FrameLabel` 仍来自 engine，用来表达当前使用哪个在飞帧槽位。
//! - 具体图像的用途、格式、bindless 可见性和 resize 生命周期属于 app 层管线策略。
//! - 本模块不保存 `Gfx` / device / allocator 引用，避免长期资源 owner 反向持有 runtime 能力。

use ash::vk;
use itertools::Itertools;
use slotmap::Key;
use truvis_gfx::commands::barrier::GfxImageBarrier;
use truvis_gfx::gfx::{GfxDeviceCtx, GfxImmediateCtx, GfxResourceCtx};
use truvis_gfx::resources::image::{GfxImage, GfxImageCreateInfo};
use truvis_gfx::resources::image_view::GfxImageViewDesc;
use truvis_gfx::resources::lifecycle::DestroyReason;
use truvis_render_foundation::frame_counter::FrameCounter;
use truvis_render_foundation::frame_counter::FrameLabel;
use truvis_render_foundation::handles::{GfxImageHandle, GfxImageViewHandle};
use truvis_render_runtime::bindings::shader_binding_system::ShaderBindingSystem;
use truvis_render_runtime::resources::gfx_resource_manager::GfxResourceManager;
use truvis_render_runtime::state::frame_state::FrameRenderState;

/// RenderGraph 导入图像所需的 handle、格式和尺寸快照。
///
/// 这里的 handle 不是资源所有权本身，而是 `GfxResourceManager` 中已注册对象的稳定索引。
/// 调用方只能在 owner 存活期间把它导入 RenderGraph；真实释放仍由 owner 在
/// resize / shutdown 阶段通过 manager 显式完成。
#[derive(Clone, Copy)]
pub struct ImageTarget {
    /// manager-owned image handle，用于 RenderGraph import。
    pub image: GfxImageHandle,
    /// 对应 image view handle；bindless 注册也以 view 为单位。
    pub view: GfxImageViewHandle,
    /// pass 创建和 RenderGraph import 必须使用同一格式，避免 view 与 pipeline attachment 不一致。
    pub format: vk::Format,
    /// target 创建时的窗口尺寸快照，供 pass 设置 viewport、dispatch size 或 copy/resolve extent。
    pub extent: vk::Extent2D,
}

/// 一组按 frame label 轮转的窗口尺寸图像。
///
/// 这类 target 会被当前 frame label 对应的 command buffer 写入；同一 label 再次复用前，
/// runtime 的 FIF timeline 已经保证上一轮提交完成。因此它适合放置单帧 RT 输出、
/// main view color 等“每个在飞帧各一份”的图像。
struct PerFrameImageSet {
    images: [GfxImageHandle; FrameCounter::fif_count()],
    views: [GfxImageViewHandle; FrameCounter::fif_count()],
    format: vk::Format,
    extent: vk::Extent2D,
}

impl PerFrameImageSet {
    fn new(
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        gfx_resource_manager: &mut GfxResourceManager,
        desc: TargetImageDesc<'_>,
        frame_counter: &FrameCounter,
    ) -> Self {
        // image 先创建为未注册的 Vulkan wrapper，方便在同一批 immediate 命令中做初始 layout 转换；
        // 转换完成后再注册到 manager，后续只通过 handle 暴露给 RenderGraph 和 bindless。
        let create_one_image = |frame_label: FrameLabel| {
            create_image(
                resource_ctx,
                desc.extent,
                desc.format,
                desc.usage,
                format!("{}-{}-{}", desc.name_prefix, frame_label, frame_counter.frame_id()),
            )
        };
        let images = FrameCounter::frame_labes().map(create_one_image);

        transition_images_to_general(immediate_ctx, &images, &format!("transfer-{}-layout", desc.name_prefix));

        // view 生命周期由 `GfxResourceManager` 跟随 image 释放。owner 只需要保存 view handle，
        // 并在销毁 image 前把 shader-visible bindless 注册撤掉。
        let image_handles = images.map(|image| gfx_resource_manager.register_image(image));
        let image_view_handles = FrameCounter::frame_labes().map(|frame_label| {
            gfx_resource_manager.get_or_create_image_view(
                device_ctx,
                image_handles[*frame_label],
                GfxImageViewDesc::new_2d(desc.format, vk::ImageAspectFlags::COLOR),
                format!("{}-{}-{}", desc.name_prefix, frame_label, frame_counter.frame_id()),
            )
        });

        Self {
            images: image_handles,
            views: image_view_handles,
            format: desc.format,
            extent: desc.extent,
        }
    }

    fn target(&self, frame_label: FrameLabel) -> ImageTarget {
        ImageTarget {
            image: self.images[*frame_label],
            view: self.views[*frame_label],
            format: self.format,
            extent: self.extent,
        }
    }

    fn register_uav(&self, shader_binding_system: &mut ShaderBindingSystem) {
        for view in &self.views {
            shader_binding_system.register_uav(*view);
        }
    }

    fn register_srv(&self, shader_binding_system: &mut ShaderBindingSystem) {
        for view in &self.views {
            shader_binding_system.register_srv(*view);
        }
    }

    fn unregister_uav(&self, shader_binding_system: &mut ShaderBindingSystem) {
        for view in &self.views {
            shader_binding_system.unregister_uav(*view);
        }
    }

    fn unregister_srv(&self, shader_binding_system: &mut ShaderBindingSystem) {
        for view in &self.views {
            shader_binding_system.unregister_srv(*view);
        }
    }

    fn destroy(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        gfx_resource_manager: &mut GfxResourceManager,
        reason: DestroyReason,
    ) {
        // 调用者必须先完成 bindless 注销；这里仅释放 manager-owned image。
        // view 会由 manager 在释放 image 时按 image-view-before-image 顺序处理。
        for image in std::mem::take(&mut self.images) {
            gfx_resource_manager.release_image_immediate(resource_ctx, device_ctx, image, reason);
        }
        self.views = Default::default();
    }
}

impl Drop for PerFrameImageSet {
    fn drop(&mut self) {
        debug_assert!(self.images.iter().all(|img| img.is_null()));
    }
}
/// 单张窗口尺寸图像。
///
/// 这类 target 不随 frame label 轮转，适合保存跨帧持续累积的 pipeline 私有历史。
/// 调用方必须通过 RenderGraph 为每次读写声明状态；本类型只负责 image/view 生命周期和
/// bindless 注册，不表达任何跨帧同步语义。
struct SingleImageTarget {
    image: GfxImageHandle,
    view: GfxImageViewHandle,
    format: vk::Format,
    extent: vk::Extent2D,
}

impl SingleImageTarget {
    fn new(
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        gfx_resource_manager: &mut GfxResourceManager,
        desc: TargetImageDesc<'_>,
        frame_counter: &FrameCounter,
    ) -> Self {
        let image = create_image(
            resource_ctx,
            desc.extent,
            desc.format,
            desc.usage,
            format!("{}-{}", desc.name_prefix, frame_counter.frame_id()),
        );
        transition_images_to_general(
            immediate_ctx,
            std::slice::from_ref(&image),
            &format!("transfer-{}-layout", desc.name_prefix),
        );

        let image_handle = gfx_resource_manager.register_image(image);
        let view_handle = gfx_resource_manager.get_or_create_image_view(
            device_ctx,
            image_handle,
            GfxImageViewDesc::new_2d(desc.format, vk::ImageAspectFlags::COLOR),
            format!("{}-{}", desc.name_prefix, frame_counter.frame_id()),
        );

        Self {
            image: image_handle,
            view: view_handle,
            format: desc.format,
            extent: desc.extent,
        }
    }

    fn target(&self) -> ImageTarget {
        ImageTarget {
            image: self.image,
            view: self.view,
            format: self.format,
            extent: self.extent,
        }
    }

    fn register_uav(&self, shader_binding_system: &mut ShaderBindingSystem) {
        shader_binding_system.register_uav(self.view);
    }

    fn register_srv(&self, shader_binding_system: &mut ShaderBindingSystem) {
        shader_binding_system.register_srv(self.view);
    }

    fn unregister_uav(&self, shader_binding_system: &mut ShaderBindingSystem) {
        shader_binding_system.unregister_uav(self.view);
    }

    fn unregister_srv(&self, shader_binding_system: &mut ShaderBindingSystem) {
        shader_binding_system.unregister_srv(self.view);
    }

    fn destroy(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        gfx_resource_manager: &mut GfxResourceManager,
        reason: DestroyReason,
    ) {
        gfx_resource_manager.release_image_immediate(resource_ctx, device_ctx, self.image, reason);
        self.image = GfxImageHandle::default();
        self.view = GfxImageViewHandle::default();
    }
}

impl Drop for SingleImageTarget {
    fn drop(&mut self) {
        debug_assert!(self.image.is_null());
        debug_assert!(self.view.is_null());
    }
}

/// RT 管线工作图像：单帧 ray tracing 输出。
///
/// `single_frame_rt` 是 per-frame target，因为 raygen 每帧写入当前 FIF 槽位。
/// SR 接入后这里保持低分辨率 render extent；历史累积和 denoise 不再是主流程的一部分。
pub struct RtWorkingTargets {
    single_frame_rt: PerFrameImageSet,
}

impl RtWorkingTargets {
    pub fn new(
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        gfx_resource_manager: &mut GfxResourceManager,
        shader_binding_system: &mut ShaderBindingSystem,
        frame_state: &FrameRenderState,
        frame_counter: &FrameCounter,
    ) -> Self {
        // RT working targets 既要作为 storage image 被 compute/ray tracing pass 写入，
        // 也可能被后续 pass 读取或 copy，因此统一保留 STORAGE / SAMPLED / TRANSFER_SRC 能力。
        let storage_usage =
            vk::ImageUsageFlags::STORAGE | vk::ImageUsageFlags::TRANSFER_SRC | vk::ImageUsageFlags::SAMPLED;
        let single_frame_rt = PerFrameImageSet::new(
            resource_ctx,
            device_ctx,
            immediate_ctx,
            gfx_resource_manager,
            TargetImageDesc {
                name_prefix: "single-frame-rt",
                format: frame_state.hdr_color_format,
                extent: frame_state.render_extent,
                usage: storage_usage,
            },
            frame_counter,
        );

        let targets = Self { single_frame_rt };
        targets.register_bindless(shader_binding_system);
        targets
    }

    pub fn rebuild(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        shader_binding_system: &mut ShaderBindingSystem,
        gfx_resource_manager: &mut GfxResourceManager,
        frame_state: &FrameRenderState,
        frame_counter: &FrameCounter,
    ) {
        // resize 走 destroy + new，而不是在原 handle 上复用；这样旧尺寸 image/view/bindless slot
        // 会按明确的 DestroyReason 离开全局表，RenderGraph 下一帧只看到新尺寸 target。
        self.destroy(resource_ctx, device_ctx, shader_binding_system, gfx_resource_manager, DestroyReason::Resize);
        *self = Self::new(
            resource_ctx,
            device_ctx,
            immediate_ctx,
            gfx_resource_manager,
            shader_binding_system,
            frame_state,
            frame_counter,
        );
    }

    pub fn destroy(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        shader_binding_system: &mut ShaderBindingSystem,
        gfx_resource_manager: &mut GfxResourceManager,
        reason: DestroyReason,
    ) {
        // bindless slot 可能仍被 shader-visible descriptor table 引用；必须先注销 view，
        // 再释放 manager image，避免后续 descriptor 更新读到已释放的 view handle。
        self.unregister_bindless(shader_binding_system);
        self.single_frame_rt.destroy(resource_ctx, device_ctx, gfx_resource_manager, reason);
    }

    #[inline]
    pub fn single_frame_rt(&self, frame_label: FrameLabel) -> ImageTarget {
        self.single_frame_rt.target(frame_label)
    }

    fn register_bindless(&self, shader_binding_system: &mut ShaderBindingSystem) {
        self.single_frame_rt.register_uav(shader_binding_system);
        self.single_frame_rt.register_srv(shader_binding_system);
    }

    fn unregister_bindless(&self, shader_binding_system: &mut ShaderBindingSystem) {
        self.single_frame_rt.unregister_srv(shader_binding_system);
        self.single_frame_rt.unregister_uav(shader_binding_system);
    }
}

impl Drop for RtWorkingTargets {
    fn drop(&mut self) {
        debug_assert!(self.single_frame_rt.images.iter().all(|img| img.is_null()));
    }
}
/// 离线 ground truth 管线的窗口尺寸图像。
///
/// `accum_image` 是唯一跨帧历史，不能按 frame label 轮转；`single_frame_image` 和
/// `render_target` 仍是 per-FIF 图像，分别服务当前采样输出和 present graph 输入。
pub struct OfflineTargets {
    single_frame_image: PerFrameImageSet,
    accum_image: SingleImageTarget,
    render_target: PerFrameImageSet,
}

impl OfflineTargets {
    pub fn new(
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        gfx_resource_manager: &mut GfxResourceManager,
        shader_binding_system: &mut ShaderBindingSystem,
        frame_state: &FrameRenderState,
        frame_counter: &FrameCounter,
    ) -> Self {
        // 三张离线图像都必须可被 compute/RT 作为 storage image 写入，并可作为 SRV
        // 暴露给 debug viewer / present path。render_target 额外带 COLOR_ATTACHMENT，是为了兼容
        // 后续可能复用的色彩/resolve 路径；当前 ownership 仍由 RenderGraph import/export 表达。
        let storage_usage =
            vk::ImageUsageFlags::STORAGE | vk::ImageUsageFlags::TRANSFER_SRC | vk::ImageUsageFlags::SAMPLED;
        let present_usage = storage_usage | vk::ImageUsageFlags::COLOR_ATTACHMENT;

        let single_frame_image = PerFrameImageSet::new(
            resource_ctx,
            device_ctx,
            immediate_ctx,
            gfx_resource_manager,
            TargetImageDesc {
                name_prefix: "offline-single-frame",
                format: frame_state.hdr_color_format,
                extent: frame_state.render_extent,
                usage: storage_usage,
            },
            frame_counter,
        );
        let accum_image = SingleImageTarget::new(
            resource_ctx,
            device_ctx,
            immediate_ctx,
            gfx_resource_manager,
            TargetImageDesc {
                name_prefix: "offline-accum",
                format: frame_state.hdr_color_format,
                extent: frame_state.render_extent,
                usage: storage_usage,
            },
            frame_counter,
        );
        let render_target = PerFrameImageSet::new(
            resource_ctx,
            device_ctx,
            immediate_ctx,
            gfx_resource_manager,
            TargetImageDesc {
                name_prefix: "offline-render-target",
                format: frame_state.hdr_color_format,
                extent: frame_state.output_extent,
                usage: present_usage,
            },
            frame_counter,
        );

        let targets = Self {
            single_frame_image,
            accum_image,
            render_target,
        };
        targets.register_bindless(shader_binding_system);
        targets
    }

    pub fn rebuild(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        shader_binding_system: &mut ShaderBindingSystem,
        gfx_resource_manager: &mut GfxResourceManager,
        frame_state: &FrameRenderState,
        frame_counter: &FrameCounter,
    ) {
        self.destroy(resource_ctx, device_ctx, shader_binding_system, gfx_resource_manager, DestroyReason::Resize);
        *self = Self::new(
            resource_ctx,
            device_ctx,
            immediate_ctx,
            gfx_resource_manager,
            shader_binding_system,
            frame_state,
            frame_counter,
        );
    }

    pub fn destroy(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        shader_binding_system: &mut ShaderBindingSystem,
        gfx_resource_manager: &mut GfxResourceManager,
        reason: DestroyReason,
    ) {
        // bindless 表项必须先撤销再释放 manager image；否则 GUI/debug 或后续 pass 可能保留
        // 指向已销毁 view 的 UAV/SRV handle。具体 image/view 释放顺序交给 manager 处理。
        self.unregister_bindless(shader_binding_system);
        self.single_frame_image.destroy(resource_ctx, device_ctx, gfx_resource_manager, reason);
        self.accum_image.destroy(resource_ctx, device_ctx, gfx_resource_manager, reason);
        self.render_target.destroy(resource_ctx, device_ctx, gfx_resource_manager, reason);
    }

    #[inline]
    pub fn single_frame_image(&self, frame_label: FrameLabel) -> ImageTarget {
        self.single_frame_image.target(frame_label)
    }

    #[inline]
    pub fn accum_image(&self) -> ImageTarget {
        self.accum_image.target()
    }

    #[inline]
    pub fn render_target(&self, frame_label: FrameLabel) -> ImageTarget {
        self.render_target.target(frame_label)
    }

    fn register_bindless(&self, shader_binding_system: &mut ShaderBindingSystem) {
        // 离线 target 同时注册 UAV/SRV：RT/compute clear/accum 走 UAV 写入，GUI/debug/present
        // 相关路径以 SRV 读取。注册集中在 owner 内，避免 pass 层临时改动全局 bindless 表。
        self.single_frame_image.register_uav(shader_binding_system);
        self.single_frame_image.register_srv(shader_binding_system);
        self.accum_image.register_uav(shader_binding_system);
        self.accum_image.register_srv(shader_binding_system);
        self.render_target.register_uav(shader_binding_system);
        self.render_target.register_srv(shader_binding_system);
    }

    fn unregister_bindless(&self, shader_binding_system: &mut ShaderBindingSystem) {
        // 注销顺序与注册顺序不承担同步语义；关键不变量是所有 shader-visible view
        // 在 image 释放前都已从 bindless 系统移除。
        self.single_frame_image.unregister_srv(shader_binding_system);
        self.single_frame_image.unregister_uav(shader_binding_system);
        self.accum_image.unregister_srv(shader_binding_system);
        self.accum_image.unregister_uav(shader_binding_system);
        self.render_target.unregister_srv(shader_binding_system);
        self.render_target.unregister_uav(shader_binding_system);
    }
}

impl Drop for OfflineTargets {
    fn drop(&mut self) {
        debug_assert!(self.single_frame_image.images.iter().all(|img| img.is_null()));
        debug_assert!(self.accum_image.image.is_null());
        debug_assert!(self.render_target.images.iter().all(|img| img.is_null()));
    }
}

/// ReSTIR DI reservoir 的四图像打包视图。
///
/// A/D 使用 uint4 保存 light sample identity 与版本；B/C 使用 float4 保存样本参数与权重统计。
/// 这种拆分让 temporal/spatial reuse 能在当前 surface 上重建有限光源候选，而不是复用旧方向。
#[derive(Clone, Copy)]
pub struct RestirReservoirTarget {
    pub a: ImageTarget,
    pub b: ImageTarget,
    pub c: ImageTarget,
    pub d: ImageTarget,
}

/// ReSTIR DI primary surface key 的三图像打包视图。
///
/// 该 history 只服务 RT pipeline 的 temporal rejection，不进入 DLSS state，
/// 因此随 ReSTIR targets 一起按 render extent 轮转。
#[derive(Clone, Copy)]
pub struct RestirSurfaceKeyTarget {
    pub a: ImageTarget,
    pub b: ImageTarget,
    pub c: ImageTarget,
}

/// Primary ReSTIR DI 的 temporal 资源。
///
/// 资源按 FIF frame label 轮转：当前 frame label 写 initial/temporal/final/surface，
/// previous frame label 的 temporal reservoir 与 surface key 作为 history 读取。owner 不保存跨帧签名，
/// history 是否可用由 RT pipeline 在构图时用 frame/reset/mode 信息决定。
pub struct RestirDiTargets {
    initial_a: PerFrameImageSet,
    initial_b: PerFrameImageSet,
    initial_c: PerFrameImageSet,
    initial_d: PerFrameImageSet,
    temporal_a: PerFrameImageSet,
    temporal_b: PerFrameImageSet,
    temporal_c: PerFrameImageSet,
    temporal_d: PerFrameImageSet,
    final_a: PerFrameImageSet,
    final_b: PerFrameImageSet,
    final_c: PerFrameImageSet,
    final_d: PerFrameImageSet,
    surface_a: PerFrameImageSet,
    surface_b: PerFrameImageSet,
    surface_c: PerFrameImageSet,
}

impl RestirDiTargets {
    /// reservoir A：light kind、light index、class mask、valid。
    pub const ID_FORMAT: vk::Format = vk::Format::R32G32B32A32_UINT;
    /// reservoir B：light-side sample 参数，例如 HDRI direction、emissive barycentric、analytic local sample。
    pub const PARAM_FORMAT: vk::Format = vk::Format::R32G32B32A32_SFLOAT;
    /// reservoir C：weight_sum、target、M、source_age；这些值参与后续 weight 归一化。
    pub const STATS_FORMAT: vk::Format = vk::Format::R32G32B32A32_SFLOAT;
    /// reservoir D：sky / emissive / analytic light 版本和 sample-key valid bit。
    pub const VERSION_FORMAT: vk::Format = vk::Format::R32G32B32A32_UINT;
    /// surface key A/B/C：position/depth、normal/roughness、base_color/metallic。
    pub const SURFACE_FORMAT: vk::Format = vk::Format::R32G32B32A32_SFLOAT;

    pub fn new(
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        gfx_resource_manager: &mut GfxResourceManager,
        _shader_binding_system: &mut ShaderBindingSystem,
        frame_state: &FrameRenderState,
        frame_counter: &FrameCounter,
    ) -> Self {
        let usage = vk::ImageUsageFlags::STORAGE | vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_SRC;
        let mut make_set = |name_prefix: &'static str, format: vk::Format| {
            PerFrameImageSet::new(
                resource_ctx,
                device_ctx,
                immediate_ctx,
                gfx_resource_manager,
                TargetImageDesc {
                    name_prefix,
                    format,
                    extent: frame_state.render_extent,
                    usage,
                },
                frame_counter,
            )
        };

        let targets = Self {
            initial_a: make_set("restir-di-initial-a", Self::ID_FORMAT),
            initial_b: make_set("restir-di-initial-b", Self::PARAM_FORMAT),
            initial_c: make_set("restir-di-initial-c", Self::STATS_FORMAT),
            initial_d: make_set("restir-di-initial-d", Self::VERSION_FORMAT),
            temporal_a: make_set("restir-di-temporal-a", Self::ID_FORMAT),
            temporal_b: make_set("restir-di-temporal-b", Self::PARAM_FORMAT),
            temporal_c: make_set("restir-di-temporal-c", Self::STATS_FORMAT),
            temporal_d: make_set("restir-di-temporal-d", Self::VERSION_FORMAT),
            final_a: make_set("restir-di-final-a", Self::ID_FORMAT),
            final_b: make_set("restir-di-final-b", Self::PARAM_FORMAT),
            final_c: make_set("restir-di-final-c", Self::STATS_FORMAT),
            final_d: make_set("restir-di-final-d", Self::VERSION_FORMAT),
            surface_a: make_set("restir-di-surface-a", Self::SURFACE_FORMAT),
            surface_b: make_set("restir-di-surface-b", Self::SURFACE_FORMAT),
            surface_c: make_set("restir-di-surface-c", Self::SURFACE_FORMAT),
        };
        // ReSTIR targets 只通过 RT pass-local push descriptor 绑定。不要注册到全局 bindless，
        // 否则每个在飞帧的 reservoir pack 都会占用 SRV/UAV slot。
        targets
    }

    pub fn rebuild(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        shader_binding_system: &mut ShaderBindingSystem,
        gfx_resource_manager: &mut GfxResourceManager,
        frame_state: &FrameRenderState,
        frame_counter: &FrameCounter,
    ) {
        self.destroy(resource_ctx, device_ctx, shader_binding_system, gfx_resource_manager, DestroyReason::Resize);
        *self = Self::new(
            resource_ctx,
            device_ctx,
            immediate_ctx,
            gfx_resource_manager,
            shader_binding_system,
            frame_state,
            frame_counter,
        );
    }

    pub fn destroy(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        _shader_binding_system: &mut ShaderBindingSystem,
        gfx_resource_manager: &mut GfxResourceManager,
        reason: DestroyReason,
    ) {
        self.initial_a.destroy(resource_ctx, device_ctx, gfx_resource_manager, reason);
        self.initial_b.destroy(resource_ctx, device_ctx, gfx_resource_manager, reason);
        self.initial_c.destroy(resource_ctx, device_ctx, gfx_resource_manager, reason);
        self.initial_d.destroy(resource_ctx, device_ctx, gfx_resource_manager, reason);
        self.temporal_a.destroy(resource_ctx, device_ctx, gfx_resource_manager, reason);
        self.temporal_b.destroy(resource_ctx, device_ctx, gfx_resource_manager, reason);
        self.temporal_c.destroy(resource_ctx, device_ctx, gfx_resource_manager, reason);
        self.temporal_d.destroy(resource_ctx, device_ctx, gfx_resource_manager, reason);
        self.final_a.destroy(resource_ctx, device_ctx, gfx_resource_manager, reason);
        self.final_b.destroy(resource_ctx, device_ctx, gfx_resource_manager, reason);
        self.final_c.destroy(resource_ctx, device_ctx, gfx_resource_manager, reason);
        self.final_d.destroy(resource_ctx, device_ctx, gfx_resource_manager, reason);
        self.surface_a.destroy(resource_ctx, device_ctx, gfx_resource_manager, reason);
        self.surface_b.destroy(resource_ctx, device_ctx, gfx_resource_manager, reason);
        self.surface_c.destroy(resource_ctx, device_ctx, gfx_resource_manager, reason);
    }

    #[inline]
    pub fn initial(&self, frame_label: FrameLabel) -> RestirReservoirTarget {
        RestirReservoirTarget {
            a: self.initial_a.target(frame_label),
            b: self.initial_b.target(frame_label),
            c: self.initial_c.target(frame_label),
            d: self.initial_d.target(frame_label),
        }
    }

    #[inline]
    pub fn temporal(&self, frame_label: FrameLabel) -> RestirReservoirTarget {
        // temporal 是唯一允许作为下一帧 history 的 reservoir。
        // final/spatial reservoir 只服务当前帧输出，不能从这里替换成 final_reservoir。
        RestirReservoirTarget {
            a: self.temporal_a.target(frame_label),
            b: self.temporal_b.target(frame_label),
            c: self.temporal_c.target(frame_label),
            d: self.temporal_d.target(frame_label),
        }
    }

    #[inline]
    pub fn final_reservoir(&self, frame_label: FrameLabel) -> RestirReservoirTarget {
        // final reservoir 是 spatial phase 的当前帧结果；InitialOnly/Temporal 模式可能只在 final
        // shade 阶段按模式读取 initial/temporal。它仍保留独立 target，便于 debug 和后续扩展。
        RestirReservoirTarget {
            a: self.final_a.target(frame_label),
            b: self.final_b.target(frame_label),
            c: self.final_c.target(frame_label),
            d: self.final_d.target(frame_label),
        }
    }

    #[inline]
    pub fn surface_key(&self, frame_label: FrameLabel) -> RestirSurfaceKeyTarget {
        // surface key 与 reservoir 使用同一 FIF label 轮转：current key 用于当前 frame 的
        // temporal/spatial/final 重建，previous key 只用于 temporal history rejection。
        RestirSurfaceKeyTarget {
            a: self.surface_a.target(frame_label),
            b: self.surface_b.target(frame_label),
            c: self.surface_c.target(frame_label),
        }
    }

    fn for_each_set(&self, mut visit: impl FnMut(&PerFrameImageSet)) {
        visit(&self.initial_a);
        visit(&self.initial_b);
        visit(&self.initial_c);
        visit(&self.initial_d);
        visit(&self.temporal_a);
        visit(&self.temporal_b);
        visit(&self.temporal_c);
        visit(&self.temporal_d);
        visit(&self.final_a);
        visit(&self.final_b);
        visit(&self.final_c);
        visit(&self.final_d);
        visit(&self.surface_a);
        visit(&self.surface_b);
        visit(&self.surface_c);
    }
}

impl Drop for RestirDiTargets {
    fn drop(&mut self) {
        self.for_each_set(|set| debug_assert!(set.images.iter().all(|img| img.is_null())));
    }
}

/// DLSS SR 所需的低分辨率输入辅助图像。
///
/// depth 与 motion vector 都由 raygen 在 render extent 下写入；ImGui 可通过 SRV
/// 查看当前 frame label 对应图像，Streamline evaluate 则使用原始 Vulkan image/view 进行 tag。
pub struct DlssSrInputTargets {
    depth: PerFrameImageSet,
    motion_vectors: PerFrameImageSet,
}

impl DlssSrInputTargets {
    pub const DEPTH_FORMAT: vk::Format = vk::Format::R32_SFLOAT;
    pub const MOTION_VECTOR_FORMAT: vk::Format = vk::Format::R32G32_SFLOAT;

    pub fn new(
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        gfx_resource_manager: &mut GfxResourceManager,
        shader_binding_system: &mut ShaderBindingSystem,
        frame_state: &FrameRenderState,
        frame_counter: &FrameCounter,
    ) -> Self {
        // raygen 通过 UAV 写入，Debug Viewer 通过 SRV 采样；Streamline evaluate 则直接使用
        // 原始 image/view handle 做 resource tag，不经过 bindless descriptor。
        let usage = vk::ImageUsageFlags::STORAGE | vk::ImageUsageFlags::SAMPLED;
        let depth = PerFrameImageSet::new(
            resource_ctx,
            device_ctx,
            immediate_ctx,
            gfx_resource_manager,
            TargetImageDesc {
                name_prefix: "dlss-depth",
                format: Self::DEPTH_FORMAT,
                extent: frame_state.render_extent,
                usage,
            },
            frame_counter,
        );
        let motion_vectors = PerFrameImageSet::new(
            resource_ctx,
            device_ctx,
            immediate_ctx,
            gfx_resource_manager,
            TargetImageDesc {
                name_prefix: "dlss-motion-vectors",
                format: Self::MOTION_VECTOR_FORMAT,
                extent: frame_state.render_extent,
                usage,
            },
            frame_counter,
        );

        let targets = Self { depth, motion_vectors };
        targets.register_bindless(shader_binding_system);
        targets
    }

    pub fn rebuild(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        shader_binding_system: &mut ShaderBindingSystem,
        gfx_resource_manager: &mut GfxResourceManager,
        frame_state: &FrameRenderState,
        frame_counter: &FrameCounter,
    ) {
        self.destroy(resource_ctx, device_ctx, shader_binding_system, gfx_resource_manager, DestroyReason::Resize);
        *self = Self::new(
            resource_ctx,
            device_ctx,
            immediate_ctx,
            gfx_resource_manager,
            shader_binding_system,
            frame_state,
            frame_counter,
        );
    }

    pub fn destroy(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        shader_binding_system: &mut ShaderBindingSystem,
        gfx_resource_manager: &mut GfxResourceManager,
        reason: DestroyReason,
    ) {
        // SR input 同时注册 UAV/SRV；销毁前必须对两个 bindless 表都撤销注册。
        self.unregister_bindless(shader_binding_system);
        self.depth.destroy(resource_ctx, device_ctx, gfx_resource_manager, reason);
        self.motion_vectors.destroy(resource_ctx, device_ctx, gfx_resource_manager, reason);
    }

    #[inline]
    pub fn depth(&self, frame_label: FrameLabel) -> ImageTarget {
        self.depth.target(frame_label)
    }

    #[inline]
    pub fn motion_vectors(&self, frame_label: FrameLabel) -> ImageTarget {
        self.motion_vectors.target(frame_label)
    }

    fn register_bindless(&self, shader_binding_system: &mut ShaderBindingSystem) {
        self.depth.register_uav(shader_binding_system);
        self.depth.register_srv(shader_binding_system);
        self.motion_vectors.register_uav(shader_binding_system);
        self.motion_vectors.register_srv(shader_binding_system);
    }

    fn unregister_bindless(&self, shader_binding_system: &mut ShaderBindingSystem) {
        self.depth.unregister_srv(shader_binding_system);
        self.depth.unregister_uav(shader_binding_system);
        self.motion_vectors.unregister_srv(shader_binding_system);
        self.motion_vectors.unregister_uav(shader_binding_system);
    }
}

impl Drop for DlssSrInputTargets {
    fn drop(&mut self) {
        debug_assert!(self.depth.images.iter().all(|img| img.is_null()));
        debug_assert!(self.motion_vectors.images.iter().all(|img| img.is_null()));
    }
}

/// DLSS SR 使用的固定手动曝光纹理。
///
/// Streamline 的 SR 插件在缺少 `kBufferTypeExposure` 时会忽略 `useAutoExposure=false`
/// 并自动启用 NGX AutoExposure。这里用一张常驻 1x1 R32_SFLOAT 图像提供手动 exposure
/// scale=1.0，保持当前 RT HDR 输入、`preExposure=1.0` 与 `exposureScale=1.0` 的同一数值空间。
pub struct DlssSrExposureTarget {
    exposure: ImageTarget,
}

impl DlssSrExposureTarget {
    pub const FORMAT: vk::Format = vk::Format::R32_SFLOAT;
    const EXTENT: vk::Extent2D = vk::Extent2D { width: 1, height: 1 };
    const VALUE: f32 = 1.0;

    pub fn new(
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        gfx_resource_manager: &mut GfxResourceManager,
        frame_counter: &FrameCounter,
    ) -> Self {
        let name = format!("dlss-sr-exposure-{}", frame_counter.frame_id());
        let image = create_image(
            resource_ctx,
            Self::EXTENT,
            Self::FORMAT,
            vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::SAMPLED,
            &name,
        );

        // 该图像从创建到销毁始终保存固定 exposure scale=1.0。初始化后保持
        // SHADER_READ_ONLY_OPTIMAL，RenderGraph 导入时也必须使用同一初始状态，避免
        // UNDEFINED layout transition 丢弃这 4 个字节的手动曝光数据。
        let exposure_bytes = Self::VALUE.to_ne_bytes();
        let stage_buffer = immediate_ctx
            .one_time_exec(|cmd| image.transfer_data(resource_ctx, cmd, &exposure_bytes), "dlss-sr-exposure-upload");
        stage_buffer.destroy(resource_ctx, DestroyReason::ScopeDrop);

        let image_handle = gfx_resource_manager.register_image(image);
        let view_handle = gfx_resource_manager.get_or_create_image_view(
            device_ctx,
            image_handle,
            GfxImageViewDesc::new_2d(Self::FORMAT, vk::ImageAspectFlags::COLOR),
            name,
        );

        Self {
            exposure: ImageTarget {
                image: image_handle,
                view: view_handle,
                format: Self::FORMAT,
                extent: Self::EXTENT,
            },
        }
    }

    pub fn destroy(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        gfx_resource_manager: &mut GfxResourceManager,
        reason: DestroyReason,
    ) {
        gfx_resource_manager.release_image_immediate(resource_ctx, device_ctx, self.exposure.image, reason);
        self.exposure = ImageTarget::default();
    }

    #[inline]
    pub fn exposure(&self) -> ImageTarget {
        self.exposure
    }
}

impl Drop for DlssSrExposureTarget {
    fn drop(&mut self) {
        debug_assert!(self.exposure.image.is_null());
        debug_assert!(self.exposure.view.is_null());
    }
}

/// DLSS Ray Reconstruction 额外需要的低分辨率输入图像。
///
/// forward/shading normal+roughness 复用现有 GBufferA；这里补齐 RR 专用的 diffuse albedo、specular
/// albedo 和 specular motion vectors。specular motion vector 由 raygen 追踪反射方向上的
/// 虚拟几何后写入，未命中时使用零向量作为保守 fallback。
pub struct DlssRrInputTargets {
    diffuse_albedo: PerFrameImageSet,
    specular_albedo: PerFrameImageSet,
    specular_motion_vectors: PerFrameImageSet,
}

impl DlssRrInputTargets {
    pub const ALBEDO_FORMAT: vk::Format = vk::Format::R16G16B16A16_SFLOAT;
    pub const SPECULAR_MOTION_VECTOR_FORMAT: vk::Format = vk::Format::R32G32_SFLOAT;

    pub fn new(
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        gfx_resource_manager: &mut GfxResourceManager,
        shader_binding_system: &mut ShaderBindingSystem,
        frame_state: &FrameRenderState,
        frame_counter: &FrameCounter,
    ) -> Self {
        let usage = vk::ImageUsageFlags::STORAGE | vk::ImageUsageFlags::SAMPLED;
        let diffuse_albedo = PerFrameImageSet::new(
            resource_ctx,
            device_ctx,
            immediate_ctx,
            gfx_resource_manager,
            TargetImageDesc {
                name_prefix: "dlss-rr-diffuse-albedo",
                format: Self::ALBEDO_FORMAT,
                extent: frame_state.render_extent,
                usage,
            },
            frame_counter,
        );
        let specular_albedo = PerFrameImageSet::new(
            resource_ctx,
            device_ctx,
            immediate_ctx,
            gfx_resource_manager,
            TargetImageDesc {
                name_prefix: "dlss-rr-specular-albedo",
                format: Self::ALBEDO_FORMAT,
                extent: frame_state.render_extent,
                usage,
            },
            frame_counter,
        );
        let specular_motion_vectors = PerFrameImageSet::new(
            resource_ctx,
            device_ctx,
            immediate_ctx,
            gfx_resource_manager,
            TargetImageDesc {
                name_prefix: "dlss-rr-specular-motion-vectors",
                format: Self::SPECULAR_MOTION_VECTOR_FORMAT,
                extent: frame_state.render_extent,
                usage,
            },
            frame_counter,
        );

        let targets = Self {
            diffuse_albedo,
            specular_albedo,
            specular_motion_vectors,
        };
        targets.register_bindless(shader_binding_system);
        targets
    }

    pub fn rebuild(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        shader_binding_system: &mut ShaderBindingSystem,
        gfx_resource_manager: &mut GfxResourceManager,
        frame_state: &FrameRenderState,
        frame_counter: &FrameCounter,
    ) {
        self.destroy(resource_ctx, device_ctx, shader_binding_system, gfx_resource_manager, DestroyReason::Resize);
        *self = Self::new(
            resource_ctx,
            device_ctx,
            immediate_ctx,
            gfx_resource_manager,
            shader_binding_system,
            frame_state,
            frame_counter,
        );
    }

    pub fn destroy(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        shader_binding_system: &mut ShaderBindingSystem,
        gfx_resource_manager: &mut GfxResourceManager,
        reason: DestroyReason,
    ) {
        self.unregister_bindless(shader_binding_system);
        self.diffuse_albedo.destroy(resource_ctx, device_ctx, gfx_resource_manager, reason);
        self.specular_albedo.destroy(resource_ctx, device_ctx, gfx_resource_manager, reason);
        self.specular_motion_vectors.destroy(resource_ctx, device_ctx, gfx_resource_manager, reason);
    }

    #[inline]
    pub fn diffuse_albedo(&self, frame_label: FrameLabel) -> ImageTarget {
        self.diffuse_albedo.target(frame_label)
    }

    #[inline]
    pub fn specular_albedo(&self, frame_label: FrameLabel) -> ImageTarget {
        self.specular_albedo.target(frame_label)
    }

    #[inline]
    pub fn specular_motion_vectors(&self, frame_label: FrameLabel) -> ImageTarget {
        self.specular_motion_vectors.target(frame_label)
    }

    fn register_bindless(&self, shader_binding_system: &mut ShaderBindingSystem) {
        self.diffuse_albedo.register_uav(shader_binding_system);
        self.diffuse_albedo.register_srv(shader_binding_system);
        self.specular_albedo.register_uav(shader_binding_system);
        self.specular_albedo.register_srv(shader_binding_system);
        self.specular_motion_vectors.register_uav(shader_binding_system);
        self.specular_motion_vectors.register_srv(shader_binding_system);
    }

    fn unregister_bindless(&self, shader_binding_system: &mut ShaderBindingSystem) {
        self.diffuse_albedo.unregister_srv(shader_binding_system);
        self.diffuse_albedo.unregister_uav(shader_binding_system);
        self.specular_albedo.unregister_srv(shader_binding_system);
        self.specular_albedo.unregister_uav(shader_binding_system);
        self.specular_motion_vectors.unregister_srv(shader_binding_system);
        self.specular_motion_vectors.unregister_uav(shader_binding_system);
    }
}

impl Drop for DlssRrInputTargets {
    fn drop(&mut self) {
        debug_assert!(self.diffuse_albedo.images.iter().all(|img| img.is_null()));
        debug_assert!(self.specular_albedo.images.iter().all(|img| img.is_null()));
        debug_assert!(self.specular_motion_vectors.images.iter().all(|img| img.is_null()));
    }
}

/// DLSS SR / DLAA / RR 共享的高分辨率 HDR 输出图像。
///
/// Streamline evaluate 写入 output extent 下的 linear HDR color；后续 tone mapping pass 再把它写入
/// main view color，保证 GUI 和 present 仍只消费最终 SDR target。SR 与 RR 只在 evaluate feature
/// 上互斥，输出资源本身由同一个 owner 管理。
pub struct DlssOutputTargets {
    color: PerFrameImageSet,
}

impl DlssOutputTargets {
    pub fn new(
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        gfx_resource_manager: &mut GfxResourceManager,
        shader_binding_system: &mut ShaderBindingSystem,
        frame_state: &FrameRenderState,
        frame_counter: &FrameCounter,
    ) -> Self {
        // DLSS output 后续作为 storage image 被 SDR pass 读取，也会在 debug viewer 中被采样。
        // TRANSFER_DST 用于 validation/debug 清理路径，TRANSFER_SRC 保留给截图或后续 copy 诊断。
        let color = PerFrameImageSet::new(
            resource_ctx,
            device_ctx,
            immediate_ctx,
            gfx_resource_manager,
            TargetImageDesc {
                name_prefix: "dlss-output",
                format: frame_state.hdr_color_format,
                extent: frame_state.output_extent,
                usage: vk::ImageUsageFlags::STORAGE
                    | vk::ImageUsageFlags::TRANSFER_SRC
                    | vk::ImageUsageFlags::TRANSFER_DST
                    | vk::ImageUsageFlags::SAMPLED,
            },
            frame_counter,
        );

        let targets = Self { color };
        targets.register_bindless(shader_binding_system);
        targets
    }

    pub fn rebuild(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        shader_binding_system: &mut ShaderBindingSystem,
        gfx_resource_manager: &mut GfxResourceManager,
        frame_state: &FrameRenderState,
        frame_counter: &FrameCounter,
    ) {
        self.destroy(resource_ctx, device_ctx, shader_binding_system, gfx_resource_manager, DestroyReason::Resize);
        *self = Self::new(
            resource_ctx,
            device_ctx,
            immediate_ctx,
            gfx_resource_manager,
            shader_binding_system,
            frame_state,
            frame_counter,
        );
    }

    pub fn destroy(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        shader_binding_system: &mut ShaderBindingSystem,
        gfx_resource_manager: &mut GfxResourceManager,
        reason: DestroyReason,
    ) {
        self.unregister_bindless(shader_binding_system);
        self.color.destroy(resource_ctx, device_ctx, gfx_resource_manager, reason);
    }

    #[inline]
    pub fn color(&self, frame_label: FrameLabel) -> ImageTarget {
        self.color.target(frame_label)
    }

    fn register_bindless(&self, shader_binding_system: &mut ShaderBindingSystem) {
        self.color.register_uav(shader_binding_system);
        self.color.register_srv(shader_binding_system);
    }

    fn unregister_bindless(&self, shader_binding_system: &mut ShaderBindingSystem) {
        self.color.unregister_srv(shader_binding_system);
        self.color.unregister_uav(shader_binding_system);
    }
}

impl Drop for DlssOutputTargets {
    fn drop(&mut self) {
        debug_assert!(self.color.images.iter().all(|img| img.is_null()));
    }
}

/// 主视图离屏目标：最终 color target 和 raster depth target。
///
/// color target 是 per-frame 的，因为 compute graph 与 present graph 会围绕当前 frame label
/// 读写它；depth target 目前是单张窗口尺寸资源，作为 raster pass 的 depth attachment。
/// 它们属于 app 的主视图策略，不进入 engine runtime-owned render state。
pub struct MainViewTargets {
    color: PerFrameImageSet,
    depth: ImageTarget,
}

impl MainViewTargets {
    pub fn new(
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        gfx_resource_manager: &mut GfxResourceManager,
        shader_binding_system: &mut ShaderBindingSystem,
        frame_state: &FrameRenderState,
        frame_counter: &FrameCounter,
    ) -> Self {
        let color = PerFrameImageSet::new(
            resource_ctx,
            device_ctx,
            immediate_ctx,
            gfx_resource_manager,
            TargetImageDesc {
                name_prefix: "main-view-color",
                format: frame_state.hdr_color_format,
                extent: frame_state.output_extent,
                usage: vk::ImageUsageFlags::STORAGE
                    | vk::ImageUsageFlags::TRANSFER_SRC
                    | vk::ImageUsageFlags::SAMPLED
                    | vk::ImageUsageFlags::COLOR_ATTACHMENT,
            },
            frame_counter,
        );
        let depth = create_depth_target(resource_ctx, device_ctx, gfx_resource_manager, frame_state, frame_counter);

        let targets = Self { color, depth };
        targets.register_bindless(shader_binding_system);
        targets
    }

    pub fn rebuild(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        shader_binding_system: &mut ShaderBindingSystem,
        gfx_resource_manager: &mut GfxResourceManager,
        frame_state: &FrameRenderState,
        frame_counter: &FrameCounter,
    ) {
        self.destroy(resource_ctx, device_ctx, shader_binding_system, gfx_resource_manager, DestroyReason::Resize);
        *self = Self::new(
            resource_ctx,
            device_ctx,
            immediate_ctx,
            gfx_resource_manager,
            shader_binding_system,
            frame_state,
            frame_counter,
        );
    }

    pub fn destroy(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        shader_binding_system: &mut ShaderBindingSystem,
        gfx_resource_manager: &mut GfxResourceManager,
        reason: DestroyReason,
    ) {
        self.unregister_bindless(shader_binding_system);
        self.color.destroy(resource_ctx, device_ctx, gfx_resource_manager, reason);
        gfx_resource_manager.release_image_immediate(resource_ctx, device_ctx, self.depth.image, reason);
        self.depth = ImageTarget::default();
    }

    #[inline]
    pub fn color(&self, frame_label: FrameLabel) -> ImageTarget {
        self.color.target(frame_label)
    }

    #[inline]
    pub fn depth(&self) -> ImageTarget {
        self.depth
    }

    fn register_bindless(&self, shader_binding_system: &mut ShaderBindingSystem) {
        // main view color 既可能被 compute/post-process 写入，也会在 present/GUI 合成阶段被读取；
        // 因此同时注册 UAV 和 SRV。depth target 只作为 attachment 使用，不进入 bindless。
        self.color.register_uav(shader_binding_system);
        self.color.register_srv(shader_binding_system);
    }

    fn unregister_bindless(&self, shader_binding_system: &mut ShaderBindingSystem) {
        self.color.unregister_uav(shader_binding_system);
        self.color.unregister_srv(shader_binding_system);
    }
}

impl Drop for MainViewTargets {
    fn drop(&mut self) {
        debug_assert!(self.depth.image.is_null());
        debug_assert!(self.depth.view.is_null());
    }
}

impl Default for ImageTarget {
    fn default() -> Self {
        Self {
            image: GfxImageHandle::default(),
            view: GfxImageViewHandle::default(),
            format: vk::Format::UNDEFINED,
            extent: vk::Extent2D::default(),
        }
    }
}

struct TargetImageDesc<'a> {
    /// 资源名用于 debug name、Tracy span 和 destroy 日志定位。
    name_prefix: &'a str,
    /// image 与 view 使用同一格式；调用方负责保证 pipeline attachment 格式匹配。
    format: vk::Format,
    /// 创建时的窗口尺寸快照，resize 后必须重建 owner。
    extent: vk::Extent2D,
    /// 由具体 target 语义决定的 Vulkan usage，不在 engine 中硬编码。
    usage: vk::ImageUsageFlags,
}

fn create_depth_target(
    resource_ctx: GfxResourceCtx<'_>,
    device_ctx: GfxDeviceCtx<'_>,
    gfx_resource_manager: &mut GfxResourceManager,
    frame_state: &FrameRenderState,
    frame_counter: &FrameCounter,
) -> ImageTarget {
    let image = create_image(
        resource_ctx,
        frame_state.output_extent,
        frame_state.depth_format,
        vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT,
        format!("main-view-depth-{}", frame_counter.frame_id()),
    );
    let image_handle = gfx_resource_manager.register_image(image);
    let view_handle = gfx_resource_manager.get_or_create_image_view(
        device_ctx,
        image_handle,
        GfxImageViewDesc::new_2d(frame_state.depth_format, vk::ImageAspectFlags::DEPTH),
        format!("main-view-depth-{}", frame_counter.frame_id()),
    );

    ImageTarget {
        image: image_handle,
        view: view_handle,
        format: frame_state.depth_format,
        extent: frame_state.output_extent,
    }
}

fn create_image(
    resource_ctx: GfxResourceCtx<'_>,
    extent: vk::Extent2D,
    format: vk::Format,
    usage: vk::ImageUsageFlags,
    name: impl AsRef<str>,
) -> GfxImage {
    let image_create_info = GfxImageCreateInfo::new_image_2d_info(extent, format, usage);
    GfxImage::new(
        resource_ctx,
        &image_create_info,
        &vk_mem::AllocationCreateInfo {
            usage: vk_mem::MemoryUsage::AutoPreferDevice,
            ..Default::default()
        },
        name.as_ref(),
    )
}

fn transition_images_to_general(immediate_ctx: GfxImmediateCtx<'_>, images: &[GfxImage], label: &str) {
    // storage/bindless target 在本项目里以 GENERAL 作为初始稳定布局，
    // 后续精确读写状态由 RenderGraph 在每帧导入后继续接管。
    immediate_ctx.one_time_exec(
        |cmd| {
            let image_barriers = images
                .iter()
                .map(|image| {
                    GfxImageBarrier::default()
                        .image(image.handle())
                        .src_mask(vk::PipelineStageFlags2::TOP_OF_PIPE, vk::AccessFlags2::empty())
                        .dst_mask(vk::PipelineStageFlags2::BOTTOM_OF_PIPE, vk::AccessFlags2::empty())
                        .layout_transfer(vk::ImageLayout::UNDEFINED, vk::ImageLayout::GENERAL)
                        .image_aspect_flag(vk::ImageAspectFlags::COLOR)
                })
                .collect_vec();

            cmd.image_memory_barrier(vk::DependencyFlags::empty(), &image_barriers);
        },
        label,
    );
}
