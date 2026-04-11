use ash::vk;
use imgui::TextureId;
use std::collections::HashMap;
use truvis_gui_backend::gui_mesh::GuiMesh;
use truvis_gui_backend::gui_pass::GuiPass;
use truvis_render_graph::render_graph::{RgImageHandle, RgImageState, RgPass, RgPassBuilder, RgPassContext};
use truvis_render_interface::handles::GfxImageViewHandle;
use truvis_renderer::render_context::RenderContext;

pub struct GuiRgPass<'a> {
    pub gui_pass: &'a GuiPass,

    pub render_context: &'a RenderContext,

    pub ui_draw_data: &'a imgui::DrawData,
    pub gui_mesh: &'a GuiMesh,
    pub tex_map: &'a HashMap<TextureId, GfxImageViewHandle>,

    pub canvas_color: RgImageHandle,
    pub canvas_extent: vk::Extent2D,
}

impl RgPass for GuiRgPass<'_> {
    fn setup(&mut self, builder: &mut RgPassBuilder) {
        builder.read_write_image(self.canvas_color, RgImageState::COLOR_ATTACHMENT_READ_WRITE);
    }

    fn execute(&self, ctx: &RgPassContext<'_>) {
        if self.ui_draw_data.total_vtx_count == 0 {
            return;
        }

        let cmd = ctx.cmd;

        let canvas_color_view_handle =
            ctx.get_image_view_handle(self.canvas_color).expect("GuiPass: canvas_color not found");
        let canvas_color_view = ctx.resource_manager.get_image_view(canvas_color_view_handle).unwrap();

        let frame_label = self.render_context.frame_counter.frame_label();
        self.gui_pass.draw(
            frame_label,
            &self.render_context.global_descriptor_sets,
            &self.render_context.bindless_manager,
            canvas_color_view.handle(),
            self.canvas_extent,
            cmd,
            self.gui_mesh,
            self.ui_draw_data,
            self.tex_map,
        );
    }
}
