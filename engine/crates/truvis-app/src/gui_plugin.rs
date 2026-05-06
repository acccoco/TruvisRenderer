//! 作为由 app 持有的 plugin 提供 ImGui 集成。

use std::collections::HashMap;

use ash::vk;
use imgui::{DrawData, TextureId, Ui};
use truvis_frame_api::input_event::{ElementState, InputEvent, MouseButton};
use truvis_frame_api::plugin::{Plugin, PluginInitCtx, PluginRenderCtx, PluginResizeCtx, PluginShutdownCtx};
use truvis_gfx::basic::color::LabelColor;
use truvis_gfx::gfx::Gfx;
use truvis_gfx::resources::image::GfxImage;
use truvis_gfx::resources::image_view::GfxImageViewDesc;
use truvis_gfx::resources::lifecycle::DestroyReason;
use truvis_gui_backend::gui_mesh::GuiMesh;
use truvis_gui_backend::gui_pass::GuiPass;
use truvis_path::TruvisPath;
use truvis_render_graph::render_graph::{
    RenderGraphBuilder, RgImageHandle, RgImageState, RgPass, RgPassBuilder, RgPassContext,
};
use truvis_render_interface::frame_counter::FrameCounter;
use truvis_render_interface::handles::{GfxImageHandle, GfxImageViewHandle};
use truvis_render_interface::render_world::RenderWorld;

const FONT_TEXTURE_ID: usize = 0;

pub struct GuiPlugin {
    imgui_ctx: imgui::Context,
    hidpi_factor: f64,
    current_ui: Option<*mut Ui>,
    draw_data: Option<*const DrawData>,

    gui_pass: Option<GuiPass>,
    gui_meshes: Option<[GuiMesh; FrameCounter::fif_count()]>,
    tex_map: HashMap<TextureId, GfxImageViewHandle>,
    fonts_image_handle: Option<GfxImageHandle>,
    fonts_image_view_handle: Option<GfxImageViewHandle>,
}

impl Default for GuiPlugin {
    fn default() -> Self {
        Self::new(1.0)
    }
}

impl GuiPlugin {
    pub fn new(hidpi_factor: f64) -> Self {
        let mut imgui_ctx = imgui::Context::create();
        imgui_ctx.set_ini_filename(None);

        {
            let style = imgui_ctx.style_mut();
            style.use_dark_colors();
            style.colors[imgui::StyleColor::WindowBg as usize] = [0.1, 0.1, 0.1, 0.9];
        }

        imgui_ctx.io_mut().display_size = [800.0, 600.0];

        Self {
            imgui_ctx,
            hidpi_factor,
            current_ui: None,
            draw_data: None,
            gui_pass: None,
            gui_meshes: None,
            tex_map: HashMap::new(),
            fonts_image_handle: None,
            fonts_image_view_handle: None,
        }
    }

    pub fn set_hidpi_factor(&mut self, factor: f64) {
        self.hidpi_factor = factor;
    }

    pub fn set_display_size(&mut self, physical_size: [u32; 2]) {
        self.imgui_ctx.io_mut().display_size = [physical_size[0] as f32, physical_size[1] as f32];
    }

    pub fn begin_frame(&mut self, delta_time: std::time::Duration) {
        self.imgui_ctx.io_mut().update_delta_time(delta_time);
        let ui = self.imgui_ctx.new_frame() as *mut Ui;
        self.current_ui = Some(ui);
        self.draw_data = None;
    }

    pub fn ui(&self) -> &Ui {
        let ui = self.current_ui.expect("GuiPlugin::ui called outside begin_frame/end_frame");
        // 指针由 imgui::Context::new_frame 生成，在 end_frame 调用
        // Context::render 之前保持有效。
        unsafe { &*ui }
    }

    pub fn end_frame(&mut self) {
        self.current_ui = None;
        self.draw_data = Some(self.imgui_ctx.render() as *const DrawData);
    }

    pub fn prepare_render_data(&mut self, ctx: &PluginRenderCtx) {
        let draw_data =
            self.draw_data.map(|ptr| unsafe { &*ptr }).expect("GuiPlugin::prepare_render_data called before end_frame");
        let frame_label = ctx.render_world.frame_counter.frame_label();
        let meshes = self.gui_meshes.as_mut().expect("GuiPlugin not initialized");

        Gfx::get().gfx_queue().begin_label("[ui-pass]create-mesh", LabelColor::COLOR_STAGE);
        {
            let mesh = &mut meshes[*frame_label];
            mesh.grow_if_needed(draw_data);
            mesh.fill_vertex_buffer(draw_data);
            mesh.fill_index_buffer(draw_data);
        }
        Gfx::get().gfx_queue().end_label();

        self.tex_map = HashMap::from([(
            imgui::TextureId::new(FONT_TEXTURE_ID),
            self.fonts_image_view_handle.expect("imgui font texture not registered"),
        )]);
    }

    pub fn contribute_passes<'a>(
        &'a self,
        graph: &mut RenderGraphBuilder<'a>,
        ctx: &'a PluginRenderCtx<'a>,
        canvas_color: RgImageHandle,
        canvas_extent: vk::Extent2D,
    ) {
        let frame_label = ctx.render_world.frame_counter.frame_label();
        graph.add_pass(
            "gui",
            GuiRenderGraphPass {
                gui_pass: self.gui_pass.as_ref().expect("GuiPlugin not initialized"),
                render_world: ctx.render_world,
                ui_draw_data: self.draw_data(),
                gui_mesh: &self.gui_meshes.as_ref().expect("GuiPlugin not initialized")[*frame_label],
                tex_map: &self.tex_map,
                canvas_color,
                canvas_extent,
            },
        );
    }

    fn draw_data(&self) -> &DrawData {
        self.draw_data.map(|ptr| unsafe { &*ptr }).expect("GuiPlugin draw data requested before end_frame")
    }

    fn init_font(&mut self, ctx: &mut PluginInitCtx) {
        let font_size = (13.0 * self.hidpi_factor) as f32;
        let font_data = std::fs::read(TruvisPath::resources_path_str("mplus-1p-regular.ttf")).unwrap();

        self.imgui_ctx.fonts().add_font(&[
            imgui::FontSource::DefaultFontData {
                config: Some(imgui::FontConfig {
                    size_pixels: font_size,
                    ..Default::default()
                }),
            },
            imgui::FontSource::TtfData {
                data: font_data.as_ref(),
                size_pixels: font_size,
                config: Some(imgui::FontConfig {
                    rasterizer_multiply: 1.75,
                    glyph_ranges: imgui::FontGlyphRanges::japanese(),
                    ..Default::default()
                }),
            },
        ]);

        self.imgui_ctx.fonts().tex_id = imgui::TextureId::new(FONT_TEXTURE_ID);

        let io = self.imgui_ctx.io_mut();
        io.font_global_scale = 1.0;
        io.config_flags |= imgui::ConfigFlags::DOCKING_ENABLE;

        let atlas_texture = self.imgui_ctx.fonts().build_rgba32_texture();
        let fonts_image =
            GfxImage::from_rgba8(atlas_texture.width, atlas_texture.height, atlas_texture.data, "imgui-fonts");
        let fonts_image_handle = ctx.render_world.gfx_resource_manager.register_image(fonts_image);
        let fonts_image_view_handle = ctx.render_world.gfx_resource_manager.get_or_create_image_view(
            fonts_image_handle,
            GfxImageViewDesc::new_2d(vk::Format::R8G8B8A8_UNORM, vk::ImageAspectFlags::COLOR),
            "imgui-fonts",
        );
        ctx.render_world.bindless_manager.register_srv(fonts_image_view_handle);

        self.fonts_image_handle = Some(fonts_image_handle);
        self.fonts_image_view_handle = Some(fonts_image_view_handle);
    }
}

impl Plugin for GuiPlugin {
    fn init(&mut self, ctx: &mut PluginInitCtx) {
        self.gui_pass =
            Some(GuiPass::new(&ctx.render_world.global_descriptor_sets, ctx.swapchain_image_info.image_format));
        self.gui_meshes = Some(FrameCounter::frame_labes().map(GuiMesh::new));
        self.init_font(ctx);
    }

    fn on_input(&mut self, event: &InputEvent) -> bool {
        let io = self.imgui_ctx.io_mut();
        match event {
            InputEvent::Resized {
                physical_width,
                physical_height,
            } => {
                io.display_size = [*physical_width as f32, *physical_height as f32];
                false
            }
            InputEvent::MouseMoved { physical_position } => {
                io.add_mouse_pos_event([physical_position[0] as f32, physical_position[1] as f32]);
                io.want_capture_mouse
            }
            InputEvent::MouseButtonInput { button, state } => {
                if let Some(mouse_button) = match button {
                    MouseButton::Left => Some(imgui::MouseButton::Left),
                    MouseButton::Right => Some(imgui::MouseButton::Right),
                    MouseButton::Middle => Some(imgui::MouseButton::Middle),
                    _ => None,
                } {
                    io.add_mouse_button_event(mouse_button, *state == ElementState::Pressed);
                }
                io.want_capture_mouse
            }
            InputEvent::MouseWheel { delta } => {
                io.add_mouse_wheel_event([0.0, *delta as f32]);
                io.want_capture_mouse
            }
            InputEvent::KeyboardInput { .. } => io.want_capture_keyboard,
            InputEvent::Other => false,
        }
    }

    fn on_resize(&mut self, ctx: &mut PluginResizeCtx) {
        let extent = ctx.render_present.swapchain_image_info().image_extent;
        self.set_display_size([extent.width, extent.height]);
    }

    fn shutdown(&mut self, ctx: &mut PluginShutdownCtx<'_>) {
        self.current_ui = None;
        self.draw_data = None;
        self.tex_map.clear();

        if let Some(view_handle) = self.fonts_image_view_handle.take() {
            ctx.render_world.bindless_manager.unregister_srv(view_handle);
        }
        if let Some(image_handle) = self.fonts_image_handle.take() {
            ctx.render_world.gfx_resource_manager.release_image_immediate(image_handle, DestroyReason::Shutdown);
        }

        self.gui_meshes.take();
        self.gui_pass.take();
    }
}

struct GuiRenderGraphPass<'a> {
    gui_pass: &'a GuiPass,
    render_world: &'a RenderWorld,
    ui_draw_data: &'a DrawData,
    gui_mesh: &'a GuiMesh,
    tex_map: &'a HashMap<TextureId, GfxImageViewHandle>,
    canvas_color: RgImageHandle,
    canvas_extent: vk::Extent2D,
}

impl RgPass for GuiRenderGraphPass<'_> {
    fn setup(&mut self, builder: &mut RgPassBuilder) {
        builder.read_write_image(self.canvas_color, RgImageState::COLOR_ATTACHMENT_READ_WRITE);
    }

    fn execute(&self, ctx: &RgPassContext<'_>) {
        if self.ui_draw_data.total_vtx_count == 0 {
            return;
        }

        let canvas_color_view_handle =
            ctx.get_image_view_handle(self.canvas_color).expect("GuiPass: canvas_color not found");
        let canvas_color_view = ctx.resource_manager.get_image_view(canvas_color_view_handle).unwrap();

        let frame_label = self.render_world.frame_counter.frame_label();
        self.gui_pass.draw(
            frame_label,
            &self.render_world.global_descriptor_sets,
            &self.render_world.bindless_manager,
            canvas_color_view.handle(),
            self.canvas_extent,
            ctx.cmd,
            self.gui_mesh,
            self.ui_draw_data,
            self.tex_map,
        );
    }
}
