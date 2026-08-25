use ash::vk;

use app_kit::subsystem::SubsystemLifecycle;
use app_render_passes::coordinate_gizmo_pass::{CoordinateGizmoPass, CoordinateGizmoRgPass};
use truvis_render_graph::render_graph::{RenderGraphBuilder, RgImageHandle};
use truvis_render_runtime::render_runtime::{
    RenderRuntimeInitCtx, RenderRuntimeRenderCtx, RenderRuntimeResizeCtx, RenderRuntimeShutdownCtx,
};

/// Truvis 主应用持有的右下角坐标轴 gizmo owner。
///
/// gizmo 只有一条 graphics pipeline，没有窗口尺寸 image、几何 buffer 或跨帧状态。具体 pass
/// 顺序仍由 `TruvisRenderApp::render` 显式决定，本类型只负责 init / resize / shutdown 阶段内
/// pipeline 与当前 present format 的生命周期对齐。
#[derive(Default)]
pub(crate) struct CoordinateGizmoRenderer {
    inner: Option<CoordinateGizmoRendererInner>,
}

struct CoordinateGizmoRendererInner {
    pass: CoordinateGizmoPass,
    present_format: vk::Format,
}

impl CoordinateGizmoRenderer {
    pub(crate) fn contribute_passes<'a>(
        &'a self,
        graph: &mut RenderGraphBuilder<'a>,
        ctx: &'a RenderRuntimeRenderCtx<'a>,
        present_image: RgImageHandle,
        present_extent: vk::Extent2D,
    ) {
        let Some(inner) = self.inner.as_ref() else {
            return;
        };

        graph.add_pass(
            "coordinate-gizmo",
            CoordinateGizmoRgPass {
                gizmo_pass: &inner.pass,
                record_ctx: ctx.record_ctx,
                present_image,
                extent: present_extent,
            },
        );
    }
}

impl SubsystemLifecycle for CoordinateGizmoRenderer {
    fn init(&mut self, ctx: &mut RenderRuntimeInitCtx<'_>) {
        let present_format = ctx.present.swapchain_image_info().image_format;
        self.inner = Some(CoordinateGizmoRendererInner::new(ctx.device_ctx, present_format, ctx.shader_binding_system));
    }

    fn on_resize(&mut self, ctx: &mut RenderRuntimeResizeCtx<'_>) {
        let present_format = ctx.present.swapchain_image_info().image_format;
        match self.inner.as_mut() {
            Some(inner) => inner.rebuild_if_needed(ctx.device_ctx, present_format, ctx.shader_binding_system),
            None => {
                self.inner =
                    Some(CoordinateGizmoRendererInner::new(ctx.device_ctx, present_format, ctx.shader_binding_system));
            }
        }
    }

    fn shutdown(&mut self, ctx: &mut RenderRuntimeShutdownCtx<'_>) {
        if let Some(inner) = self.inner.take() {
            inner.destroy(ctx.device_ctx);
        }
    }
}

impl CoordinateGizmoRendererInner {
    fn new(
        device_ctx: truvis_gfx::gfx::GfxDeviceCtx<'_>,
        present_format: vk::Format,
        shader_binding_system: &truvis_render_runtime::bindings::shader_binding_system::ShaderBindingSystem,
    ) -> Self {
        let pass = CoordinateGizmoPass::new(device_ctx, present_format, shader_binding_system.global_descriptor_sets());
        Self { pass, present_format }
    }

    fn rebuild_if_needed(
        &mut self,
        device_ctx: truvis_gfx::gfx::GfxDeviceCtx<'_>,
        present_format: vk::Format,
        shader_binding_system: &truvis_render_runtime::bindings::shader_binding_system::ShaderBindingSystem,
    ) {
        if self.present_format == present_format {
            return;
        }

        // color attachment format 是 graphics pipeline 创建契约的一部分；swapchain format 变化时必须重建。
        let new_pass =
            CoordinateGizmoPass::new(device_ctx, present_format, shader_binding_system.global_descriptor_sets());
        let old_pass = std::mem::replace(&mut self.pass, new_pass);
        old_pass.destroy(device_ctx);
        self.present_format = present_format;
    }

    fn destroy(self, device_ctx: truvis_gfx::gfx::GfxDeviceCtx<'_>) {
        self.pass.destroy(device_ctx);
    }
}
