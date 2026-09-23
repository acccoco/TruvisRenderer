use ash::vk;

use renderer_kit::subsystem::SubsystemLifecycle;
use renderer_render_passes::effects::viewport_overlay::{
    ViewportOverlayPass, ViewportOverlayRgPass, ViewportOverlayVertex, ViewportOverlayVertexLayout,
};
use truvis_gfx::{
    gfx::GfxResourceCtx,
    resources::{lifecycle::DestroyReason, special_buffers::vertex_buffer::GfxVertexBuffer},
};
use truvis_render_foundation::frame_label::FrameLabel;
use truvis_render_foundation::render_view::RenderView;
use truvis_render_graph::render_graph::{RenderGraphBuilder, RgImageHandle};
use truvis_render_runtime::render_runtime::{
    RenderRuntimeInitCtx, RenderRuntimeRenderCtx, RenderRuntimeResizeCtx, RenderRuntimeShutdownCtx,
};
use truvis_world::LightTarget;

use crate::{light_overlay::LightOverlay, overlay_geometry::OverlayGeometry, transform_gizmo::TransformGizmo};

/// 所有 viewport 辅助图形的唯一 GPU owner；不持有 World 或交互状态。
#[derive(Default)]
pub(crate) struct ViewportOverlaySubsystem {
    resources: Option<ViewportOverlayResources>,
    vertices: Vec<ViewportOverlayVertex>,
    vertex_count: u32,
}

struct ViewportOverlayResources {
    pass: ViewportOverlayPass,
    format: vk::Format,
    /// 当前 frame label 已由 Runtime 等待；只写或扩容该副本，不改仍在飞行中的 buffer。
    buffers: [GfxVertexBuffer<ViewportOverlayVertexLayout>; FrameLabel::COUNT],
}

impl ViewportOverlaySubsystem {
    pub(crate) fn prepare_render(
        &mut self,
        ctx: &RenderRuntimeRenderCtx<'_>,
        view: &RenderView,
        lights: &LightOverlay,
        gizmo: &TransformGizmo,
        selected: Option<LightTarget>,
    ) {
        self.vertices.clear();
        self.vertex_count = 0;
        let extent = ctx.present.swapchain_image_info().image_extent;
        if extent.width == 0 || extent.height == 0 {
            return;
        }
        let viewport = glam::vec2(extent.width as f32, extent.height as f32);
        let mut geometry = OverlayGeometry::new(viewport, &mut self.vertices);
        lights.append_geometry(&mut geometry, selected);
        gizmo.append_geometry(&mut geometry);
        geometry.coordinate_gizmo(view);
        let Some(resources) = self.resources.as_mut() else {
            return;
        };
        self.vertex_count = self.vertices.len() as u32;
        if self.vertex_count == 0 {
            return;
        }
        let label = ctx.record_ctx.frame_timing.frame_label();
        let buffer = &mut resources.buffers[*label];
        if buffer.vertex_cnt() < self.vertices.len() {
            let capacity = self.vertices.len().next_power_of_two();
            log::debug!("Viewport overlay buffer {label}: {} -> {capacity} vertices", buffer.vertex_cnt());
            buffer.destroy_mut(ctx.resource_ctx, DestroyReason::ImmediateRelease);
            *buffer = Self::new_buffer(ctx.resource_ctx, label, capacity);
        }
        buffer.transfer_data_by_mmap(ctx.resource_ctx, &self.vertices);
    }

    pub(crate) fn contribute_passes<'a>(
        &'a self,
        graph: &mut RenderGraphBuilder<'a>,
        ctx: &RenderRuntimeRenderCtx<'_>,
        present_image: RgImageHandle,
    ) {
        let Some(resources) = self.resources.as_ref() else {
            return;
        };
        let extent = ctx.present.swapchain_image_info().image_extent;
        if self.vertex_count == 0 || extent.width == 0 || extent.height == 0 {
            return;
        }
        let label = ctx.record_ctx.frame_timing.frame_label();
        graph.add_pass(
            "viewport-overlay",
            ViewportOverlayRgPass {
                pass: &resources.pass,
                present_image,
                extent,
                vertices: resources.buffers[*label].vk_buffer(),
                vertex_count: self.vertex_count,
            },
        );
    }

    fn new_buffer(
        ctx: GfxResourceCtx<'_>,
        label: FrameLabel,
        capacity: usize,
    ) -> GfxVertexBuffer<ViewportOverlayVertexLayout> {
        GfxVertexBuffer::new(ctx, capacity, true, format!("viewport-overlay-{label}"))
    }
}

impl SubsystemLifecycle for ViewportOverlaySubsystem {
    fn init(&mut self, ctx: &mut RenderRuntimeInitCtx<'_>) {
        let format = ctx.present.swapchain_image_info().image_format;
        self.resources = Some(ViewportOverlayResources {
            pass: ViewportOverlayPass::new(ctx.device_ctx, format),
            format,
            buffers: FrameLabel::ALL.map(|label| Self::new_buffer(ctx.resource_ctx, label, 2048)),
        });
    }
    fn on_resize(&mut self, ctx: &mut RenderRuntimeResizeCtx<'_>) {
        if let Some(resources) = self.resources.as_mut() {
            let format = ctx.present.swapchain_image_info().image_format;
            if resources.format != format {
                let old = std::mem::replace(&mut resources.pass, ViewportOverlayPass::new(ctx.device_ctx, format));
                old.destroy(ctx.device_ctx);
                resources.format = format;
            }
        }
    }
    fn shutdown(&mut self, ctx: &mut RenderRuntimeShutdownCtx<'_>) {
        if let Some(mut resources) = self.resources.take() {
            for buffer in &mut resources.buffers {
                buffer.destroy_mut(ctx.resource_ctx, DestroyReason::Shutdown);
            }
            resources.pass.destroy(ctx.device_ctx);
        }
    }
}
