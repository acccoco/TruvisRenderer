use ash::vk;
use slotmap::Key;

use app_kit::subsystem::SubsystemLifecycle;
use app_render_passes::effects::selection_outline::{
    SelectionOutlineCompositeRgPass, SelectionOutlineMaskRgPass, SelectionOutlinePass,
};
use app_rendering::ImageTarget;
use truvis_gfx::gfx::{GfxDeviceCtx, GfxResourceCtx};
use truvis_gfx::resources::image::{GfxImage, GfxImageCreateInfo};
use truvis_gfx::resources::image_view::GfxImageViewDesc;
use truvis_gfx::resources::lifecycle::DestroyReason;
use truvis_render_foundation::frame_label::FrameLabel;
use truvis_render_foundation::handles::{GfxImageHandle, GfxImageViewHandle};
use truvis_render_graph::render_graph::{RenderGraphBuilder, RgImageHandle, RgImageState};
use truvis_render_runtime::bindings::global_descriptor_sets::GlobalDescriptorSets;
use truvis_render_runtime::render_runtime::{
    RenderRuntimeInitCtx, RenderRuntimeRenderCtx, RenderRuntimeResizeCtx, RenderRuntimeShutdownCtx,
};
use truvis_render_runtime::resources::gfx_resource_manager::GfxResourceManager;
use truvis_render_runtime::selection::WorldSubmeshSelection;

/// Truvis app 拥有的 selection outline 资源 owner。
///
/// 本类型只持有窗口尺寸 mask image 和 outline pass pipeline，不进入 engine runtime。
/// mask 按 FIF 轮转，跟随 swapchain/output extent 在 init/resize 阶段创建或重建，
/// composite pass 在录制时通过 pass-local descriptor 采样 mask；shutdown 只释放 manager-owned image。
#[derive(Default)]
pub(crate) struct SelectionOutlineSubsystem {
    resources: Option<SelectionOutlineResources>,
}

struct SelectionOutlineResources {
    pass: SelectionOutlinePass,
    present_format: vk::Format,
    masks: SelectionOutlineMasks,
}

/// selection outline 的 per-FIF mask image 集合。
///
/// mask 是 app-owned 窗口尺寸资源，只用于最终主视图合成；它不会注册成 GUI debug image。
/// 每帧 mask pass 会 clear 当前 frame label 的 image，因此跨帧内容不承担语义。
struct SelectionOutlineMasks {
    images: [GfxImageHandle; FrameLabel::COUNT],
    views: [GfxImageViewHandle; FrameLabel::COUNT],
    extent: vk::Extent2D,
}

impl SelectionOutlineSubsystem {
    pub(crate) fn contribute_passes<'a>(
        &'a self,
        graph: &mut RenderGraphBuilder<'a>,
        ctx: &'a RenderRuntimeRenderCtx<'a>,
        present_image: RgImageHandle,
        present_extent: vk::Extent2D,
        selection: Option<WorldSubmeshSelection>,
    ) {
        let Some(selection) = selection else {
            return;
        };
        let Some(resources) = self.resources.as_ref() else {
            return;
        };

        let frame_label = ctx.record_ctx.frame_timing.frame_label();
        let mask_target = resources.masks.target(frame_label);
        let mask_image = graph.import_image(
            "selection-outline-mask",
            mask_target.image,
            Some(mask_target.view),
            mask_target.format,
            RgImageState::UNDEFINED_TOP,
            None,
        );

        graph.add_pass(
            "selection-outline-mask",
            SelectionOutlineMaskRgPass {
                outline_pass: &resources.pass,
                record_ctx: ctx.record_ctx,
                selected_raster: ctx.world_submesh_raster,
                selection,
                mask_image,
                extent: present_extent,
            },
        );
        graph.add_pass(
            "selection-outline-composite",
            SelectionOutlineCompositeRgPass {
                outline_pass: &resources.pass,
                record_ctx: ctx.record_ctx,
                mask_image,
                present_image,
                extent: present_extent,
            },
        );
    }
}

impl SubsystemLifecycle for SelectionOutlineSubsystem {
    fn init(&mut self, ctx: &mut RenderRuntimeInitCtx<'_>) {
        let image_info = ctx.present.swapchain_image_info();
        let resources = SelectionOutlineResources::new(
            ctx.resource_ctx,
            ctx.device_ctx,
            ctx.gfx_resource_manager,
            ctx.shader_binding_system.global_descriptor_sets(),
            image_info.image_extent,
            image_info.image_format,
            ctx.frame_timing.frame_id(),
        );
        self.resources = Some(resources);
    }

    fn on_resize(&mut self, ctx: &mut RenderRuntimeResizeCtx<'_>) {
        let image_info = ctx.present.swapchain_image_info();
        if let Some(resources) = self.resources.as_mut() {
            resources.rebuild_masks(
                ctx.resource_ctx,
                ctx.device_ctx,
                ctx.gfx_resource_manager,
                ctx.shader_binding_system.global_descriptor_sets(),
                image_info.image_extent,
                image_info.image_format,
                ctx.frame_timing.frame_id(),
            );
        } else {
            self.resources = Some(SelectionOutlineResources::new(
                ctx.resource_ctx,
                ctx.device_ctx,
                ctx.gfx_resource_manager,
                ctx.shader_binding_system.global_descriptor_sets(),
                image_info.image_extent,
                image_info.image_format,
                ctx.frame_timing.frame_id(),
            ));
        }
    }

    fn shutdown(&mut self, ctx: &mut RenderRuntimeShutdownCtx<'_>) {
        if let Some(resources) = self.resources.take() {
            resources.destroy(ctx.resource_ctx, ctx.device_ctx, ctx.gfx_resource_manager, DestroyReason::Shutdown);
        }
    }
}

impl SelectionOutlineResources {
    fn new(
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        gfx_resource_manager: &mut GfxResourceManager,
        global_descriptor_sets: &GlobalDescriptorSets,
        extent: vk::Extent2D,
        present_format: vk::Format,
        frame_id: u64,
    ) -> Self {
        let pass = SelectionOutlinePass::new(device_ctx, present_format, global_descriptor_sets);
        let masks = SelectionOutlineMasks::new(resource_ctx, device_ctx, gfx_resource_manager, extent, frame_id);
        Self {
            pass,
            present_format,
            masks,
        }
    }

    fn rebuild_masks(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        gfx_resource_manager: &mut GfxResourceManager,
        global_descriptor_sets: &GlobalDescriptorSets,
        extent: vk::Extent2D,
        present_format: vk::Format,
        frame_id: u64,
    ) {
        if self.present_format != present_format {
            // composite pipeline 的 color attachment format 必须和当前 swapchain/present format 对齐。
            let new_pass = SelectionOutlinePass::new(device_ctx, present_format, global_descriptor_sets);
            let old_pass = std::mem::replace(&mut self.pass, new_pass);
            old_pass.destroy(device_ctx);
            self.present_format = present_format;
        }

        self.masks.destroy(resource_ctx, device_ctx, gfx_resource_manager, DestroyReason::Resize);
        self.masks = SelectionOutlineMasks::new(resource_ctx, device_ctx, gfx_resource_manager, extent, frame_id);
    }

    fn destroy(
        self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        gfx_resource_manager: &mut GfxResourceManager,
        reason: DestroyReason,
    ) {
        let Self {
            pass,
            present_format: _,
            mut masks,
        } = self;
        masks.destroy(resource_ctx, device_ctx, gfx_resource_manager, reason);
        pass.destroy(device_ctx);
    }
}

impl SelectionOutlineMasks {
    fn new(
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        gfx_resource_manager: &mut GfxResourceManager,
        extent: vk::Extent2D,
        frame_id: u64,
    ) -> Self {
        let images = FrameLabel::ALL.map(|frame_label| {
            let image = Self::create_mask_image(
                resource_ctx,
                extent,
                format!("selection-outline-mask-{}-{}", frame_label, frame_id),
            );
            gfx_resource_manager.register_image(image)
        });
        let views = FrameLabel::ALL.map(|frame_label| {
            gfx_resource_manager.get_or_create_image_view(
                device_ctx,
                images[*frame_label],
                GfxImageViewDesc::new_2d(SelectionOutlinePass::MASK_FORMAT, vk::ImageAspectFlags::COLOR),
                format!("selection-outline-mask-{}-{}", frame_label, frame_id),
            )
        });

        Self { images, views, extent }
    }

    fn target(&self, frame_label: FrameLabel) -> ImageTarget {
        ImageTarget {
            image: self.images[*frame_label],
            view: self.views[*frame_label],
            format: SelectionOutlinePass::MASK_FORMAT,
            extent: self.extent,
        }
    }

    fn destroy(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        gfx_resource_manager: &mut GfxResourceManager,
        reason: DestroyReason,
    ) {
        for image in std::mem::take(&mut self.images) {
            gfx_resource_manager.release_image_immediate(resource_ctx, device_ctx, image, reason);
        }
        self.views = Default::default();
    }

    fn create_mask_image(resource_ctx: GfxResourceCtx<'_>, extent: vk::Extent2D, name: impl AsRef<str>) -> GfxImage {
        let image_create_info = GfxImageCreateInfo::new_image_2d_info(
            extent,
            SelectionOutlinePass::MASK_FORMAT,
            vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::SAMPLED,
        );
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
}

impl Drop for SelectionOutlineMasks {
    fn drop(&mut self) {
        debug_assert!(self.images.iter().all(|image| image.is_null()));
        debug_assert!(self.views.iter().all(|view| view.is_null()));
    }
}
