//! 作为由 app 持有的 plugin 提供 ImGui 集成。

use std::cell::Cell;
use std::collections::HashMap;

use crate::gui_backend::gui_mesh::GuiMesh;
use crate::gui_backend::gui_pass::GuiPass;
use ash::vk;
use imgui::{DrawData, TextureId, Ui};
use slotmap::Key;
use truvis_app_frame::input_event::{ElementState, InputEvent, MouseButton};
use truvis_app_frame::plugin_api::{Plugin, PluginInitCtx, PluginRenderCtx, PluginResizeCtx, PluginShutdownCtx};
use truvis_gfx::basic::color::LabelColor;
use truvis_gfx::resources::image::GfxImage;
use truvis_gfx::resources::image_view::GfxImageViewDesc;
use truvis_gfx::resources::lifecycle::DestroyReason;
use truvis_path::TruvisPath;
use truvis_render_foundation::frame_counter::FrameCounter;
use truvis_render_foundation::gpu_store::GpuStore;
use truvis_render_foundation::handles::{GfxImageHandle, GfxImageViewHandle};
use truvis_render_graph::render_graph::{
    RenderGraphBuilder, RgImageHandle, RgImageState, RgPass, RgPassBuilder, RgPassContext,
};

const FONT_TEXTURE_ID: usize = 0;
const DEBUG_TEXTURE_ID_BASE: usize = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DebugImageVisualizeMode {
    Raw,
}

/// ImGui debug viewer 每帧显示的外部图像入口。
///
/// 该结构只保存 `GfxResourceManager` 中已有 image/view 的 handle 快照，不拥有资源生命周期。
/// 调用方必须保证图像 owner 至少活到当前 RenderGraph 录制和提交完成，并为 `view` 注册 SRV。
/// `graph_state` 描述图像跨 graph 传入 GUI preview 时的稳定状态；SR 输入被 DLSS pass 读取后
/// 可能不再是 `GENERAL`，因此不能在 GUI 侧统一假设 storage layout。
#[derive(Clone, Copy, Debug)]
pub struct DebugImageEntry {
    pub id: &'static str,
    pub label: &'static str,
    pub image: GfxImageHandle,
    pub view: GfxImageViewHandle,
    pub format: vk::Format,
    pub extent: vk::Extent2D,
    /// 该 debug image 导入当前 present graph 时的初始状态，也是预览后导出的最终状态。
    pub graph_state: RgImageState,
    pub visualize_mode: DebugImageVisualizeMode,
}

impl DebugImageEntry {
    pub const fn raw(
        id: &'static str,
        label: &'static str,
        image: GfxImageHandle,
        view: GfxImageViewHandle,
        format: vk::Format,
        extent: vk::Extent2D,
    ) -> Self {
        // 旧 debug image 默认都是 storage/bindless target，跨 graph 稳定状态为 GENERAL。
        Self::raw_with_graph_state(id, label, image, view, format, extent, RgImageState::GENERAL)
    }

    pub const fn raw_with_graph_state(
        id: &'static str,
        label: &'static str,
        image: GfxImageHandle,
        view: GfxImageViewHandle,
        format: vk::Format,
        extent: vk::Extent2D,
        graph_state: RgImageState,
    ) -> Self {
        Self {
            id,
            label,
            image,
            view,
            format,
            extent,
            graph_state,
            visualize_mode: DebugImageVisualizeMode::Raw,
        }
    }
}

/// 已导入当前 RenderGraph 的 debug image。
///
/// 用于避免同一物理图像在一个图内重复 import；例如 main view color 已经被 resolve pass
/// 导入时，GUI debug 预览必须复用同一个 `RgImageHandle`。
#[derive(Clone, Copy, Debug)]
pub struct DebugImageGraphEntry {
    pub id: &'static str,
    pub image: RgImageHandle,
    pub final_state: RgImageState,
}

impl DebugImageGraphEntry {
    pub const fn new(id: &'static str, image: RgImageHandle, final_state: RgImageState) -> Self {
        Self { id, image, final_state }
    }
}

pub struct GuiPlugin {
    imgui_ctx: imgui::Context,
    hidpi_factor: f64,
    current_ui: Option<*mut Ui>,
    draw_data: Option<*const DrawData>,

    gui_pass: Option<GuiPass>,
    gui_meshes: Option<[GuiMesh; FrameCounter::fif_count()]>,
    tex_map: HashMap<TextureId, GfxImageViewHandle>,
    debug_images: Vec<DebugImageEntry>,
    debug_texture_ids: HashMap<&'static str, TextureId>,
    selected_debug_image_id: Cell<Option<&'static str>>,
    next_debug_texture_id: usize,
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
            debug_images: Vec::new(),
            debug_texture_ids: HashMap::new(),
            selected_debug_image_id: Cell::new(None),
            next_debug_texture_id: DEBUG_TEXTURE_ID_BASE,
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

    pub fn begin_debug_image_frame(&mut self) {
        self.debug_images.clear();
    }

    pub fn register_debug_image(&mut self, entry: DebugImageEntry) {
        if entry.image.is_null() || entry.view.is_null() {
            log::warn!("GuiPlugin: skip null debug image {}", entry.id);
            return;
        }

        self.debug_texture_id(entry.id);
        if self.debug_images.iter().any(|registered| registered.id == entry.id) {
            log::warn!("GuiPlugin: replace duplicated debug image {}", entry.id);
            self.debug_images.retain(|registered| registered.id != entry.id);
        }
        if self.selected_debug_image_id.get().is_none() {
            self.selected_debug_image_id.set(Some(entry.id));
        }
        self.debug_images.push(entry);
    }

    pub fn build_debug_image_viewer_ui(&self, ui: &Ui) {
        ui.window("Debug Images")
            .position([370.0, 10.0], imgui::Condition::FirstUseEver)
            .size([420.0, 360.0], imgui::Condition::FirstUseEver)
            .build(|| {
                if self.debug_images.is_empty() {
                    ui.text("No debug image");
                    return;
                }

                self.ensure_selected_debug_image();
                let selected_id = self.selected_debug_image_id.get();
                let preview = selected_id
                    .and_then(|id| self.debug_images.iter().find(|entry| entry.id == id))
                    .map(|entry| entry.label)
                    .unwrap_or("None");

                if let Some(_combo) = ui.begin_combo("Image", preview) {
                    for entry in &self.debug_images {
                        let selected = Some(entry.id) == self.selected_debug_image_id.get();
                        if ui.selectable_config(entry.label).selected(selected).build() {
                            self.selected_debug_image_id.set(Some(entry.id));
                        }
                        if selected {
                            ui.set_item_default_focus();
                        }
                    }
                }

                let Some(entry) = self.selected_debug_entry() else {
                    ui.text("No selection");
                    return;
                };
                let texture_id = self.debug_texture_ids.get(entry.id).copied().expect("debug texture id missing");
                ui.text(format!(
                    "{} | {:?} | {}x{}",
                    entry.label, entry.format, entry.extent.width, entry.extent.height
                ));
                let size = Self::debug_image_preview_size(entry.extent, ui.content_region_avail()[0]);
                imgui::Image::new(texture_id, size).uv0([0.0, 0.0]).uv1([1.0, 1.0]).build(ui);
            });
    }

    pub fn end_frame(&mut self) {
        self.current_ui = None;
        self.draw_data = Some(self.imgui_ctx.render() as *const DrawData);
    }

    pub fn prepare_render_data(&mut self, ctx: &PluginRenderCtx) {
        let draw_data =
            self.draw_data.map(|ptr| unsafe { &*ptr }).expect("GuiPlugin::prepare_render_data called before end_frame");
        let frame_label = ctx.gpu_store.frame_counter.frame_label();
        let meshes = self.gui_meshes.as_mut().expect("GuiPlugin not initialized");

        ctx.queue_ctx.gfx_queue().begin_label("[ui-pass]create-mesh", LabelColor::COLOR_STAGE);
        {
            let mesh = &mut meshes[*frame_label];
            mesh.grow_if_needed(ctx.resource_ctx, draw_data);
            mesh.fill_vertex_buffer(ctx.resource_ctx, draw_data);
            mesh.fill_index_buffer(ctx.resource_ctx, draw_data);
        }
        ctx.queue_ctx.gfx_queue().end_label();

        let mut tex_map = HashMap::from([(
            imgui::TextureId::new(FONT_TEXTURE_ID),
            self.fonts_image_view_handle.expect("imgui font texture not registered"),
        )]);
        for entry in &self.debug_images {
            if let Some(texture_id) = self.debug_texture_ids.get(entry.id) {
                tex_map.insert(*texture_id, entry.view);
            }
        }
        self.tex_map = tex_map;
    }

    pub fn contribute_passes<'a>(
        &'a self,
        graph: &mut RenderGraphBuilder<'a>,
        ctx: &'a PluginRenderCtx<'a>,
        canvas_color: RgImageHandle,
        canvas_extent: vk::Extent2D,
        imported_debug_images: &[DebugImageGraphEntry],
    ) {
        let frame_label = ctx.gpu_store.frame_counter.frame_label();
        let debug_image = self.selected_debug_graph_image(graph, imported_debug_images);
        if let Some(debug_image) = debug_image {
            graph.export_image(debug_image.image, debug_image.final_state, None);
        }
        graph.add_pass(
            "gui",
            GuiRenderGraphPass {
                gui_pass: self.gui_pass.as_ref().expect("GuiPlugin not initialized"),
                gpu_store: ctx.gpu_store,
                ui_draw_data: self.draw_data(),
                gui_mesh: &self.gui_meshes.as_ref().expect("GuiPlugin not initialized")[*frame_label],
                tex_map: &self.tex_map,
                canvas_color,
                canvas_extent,
                debug_image: debug_image.map(|entry| entry.image),
            },
        );
    }

    fn draw_data(&self) -> &DrawData {
        self.draw_data.map(|ptr| unsafe { &*ptr }).expect("GuiPlugin draw data requested before end_frame")
    }

    fn debug_texture_id(&mut self, id: &'static str) -> TextureId {
        if let Some(texture_id) = self.debug_texture_ids.get(id) {
            return *texture_id;
        }

        let texture_id = TextureId::new(self.next_debug_texture_id);
        self.next_debug_texture_id += 1;
        self.debug_texture_ids.insert(id, texture_id);
        texture_id
    }

    fn ensure_selected_debug_image(&self) {
        let selected_is_valid =
            self.selected_debug_image_id.get().is_some_and(|id| self.debug_images.iter().any(|entry| entry.id == id));
        if !selected_is_valid {
            self.selected_debug_image_id.set(self.debug_images.first().map(|entry| entry.id));
        }
    }

    fn selected_debug_entry(&self) -> Option<&DebugImageEntry> {
        self.ensure_selected_debug_image();
        let selected_id = self.selected_debug_image_id.get()?;
        self.debug_images.iter().find(|entry| entry.id == selected_id)
    }

    fn selected_debug_graph_image(
        &self,
        graph: &mut RenderGraphBuilder<'_>,
        imported_debug_images: &[DebugImageGraphEntry],
    ) -> Option<DebugImageGraphEntry> {
        let selected_id = self.selected_debug_image_id.get()?;
        if let Some(imported) = imported_debug_images.iter().find(|entry| entry.id == selected_id) {
            return Some(*imported);
        }

        let entry = self.debug_images.iter().find(|entry| entry.id == selected_id)?;
        // 未被 present graph 其它 pass 导入的 debug image，在 GUI 侧按 owner 声明的稳定状态导入。
        // 这保证 SR 输入的 SHADER_READ_ONLY layout 不会被错误当作 GENERAL 重新声明。
        let image =
            graph.import_image(entry.label, entry.image, Some(entry.view), entry.format, entry.graph_state, None);
        Some(DebugImageGraphEntry::new(entry.id, image, entry.graph_state))
    }

    fn debug_image_preview_size(extent: vk::Extent2D, available_width: f32) -> [f32; 2] {
        let src_width = extent.width.max(1) as f32;
        let src_height = extent.height.max(1) as f32;
        let max_width = available_width.max(160.0).min(720.0);
        let max_height = 420.0;
        let scale = (max_width / src_width).min(max_height / src_height).min(1.0);
        [(src_width * scale).max(1.0), (src_height * scale).max(1.0)]
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
        let fonts_image = GfxImage::from_rgba8(
            ctx.resource_ctx,
            ctx.immediate_ctx,
            atlas_texture.width,
            atlas_texture.height,
            atlas_texture.data,
            "imgui-fonts",
        );
        let fonts_image_handle = ctx.gpu_store.gfx_resource_manager.register_image(fonts_image);
        let fonts_image_view_handle = ctx.gpu_store.gfx_resource_manager.get_or_create_image_view(
            ctx.device_ctx,
            fonts_image_handle,
            GfxImageViewDesc::new_2d(vk::Format::R8G8B8A8_UNORM, vk::ImageAspectFlags::COLOR),
            "imgui-fonts",
        );
        ctx.gpu_store.bindless_manager.register_srv(fonts_image_view_handle);

        self.fonts_image_handle = Some(fonts_image_handle);
        self.fonts_image_view_handle = Some(fonts_image_view_handle);
    }
}

impl Plugin for GuiPlugin {
    fn init(&mut self, ctx: &mut PluginInitCtx) {
        self.gui_pass = Some(GuiPass::new(
            ctx.device_ctx,
            &ctx.gpu_store.global_descriptor_sets,
            ctx.swapchain_image_info.image_format,
        ));
        self.gui_meshes =
            Some(FrameCounter::frame_labes().map(|frame_label| GuiMesh::new(ctx.resource_ctx, frame_label)));
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
        let extent = ctx.present.swapchain_image_info().image_extent;
        self.set_display_size([extent.width, extent.height]);
    }

    fn shutdown(&mut self, ctx: &mut PluginShutdownCtx<'_>) {
        self.current_ui = None;
        self.draw_data = None;
        self.tex_map.clear();
        self.debug_images.clear();
        self.debug_texture_ids.clear();
        self.selected_debug_image_id.set(None);

        if let Some(view_handle) = self.fonts_image_view_handle.take() {
            ctx.gpu_store.bindless_manager.unregister_srv(view_handle);
        }
        if let Some(image_handle) = self.fonts_image_handle.take() {
            ctx.gpu_store.gfx_resource_manager.release_image_immediate(
                ctx.resource_ctx,
                ctx.device_ctx,
                image_handle,
                DestroyReason::Shutdown,
            );
        }

        if let Some(mut meshes) = self.gui_meshes.take() {
            for mesh in &mut meshes {
                mesh.destroy_mut(ctx.resource_ctx);
            }
        }
        if let Some(gui_pass) = self.gui_pass.take() {
            gui_pass.destroy(ctx.device_ctx);
        }
    }
}

struct GuiRenderGraphPass<'a> {
    gui_pass: &'a GuiPass,
    gpu_store: &'a GpuStore,
    ui_draw_data: &'a DrawData,
    gui_mesh: &'a GuiMesh,
    tex_map: &'a HashMap<TextureId, GfxImageViewHandle>,
    canvas_color: RgImageHandle,
    canvas_extent: vk::Extent2D,
    debug_image: Option<RgImageHandle>,
}

impl RgPass for GuiRenderGraphPass<'_> {
    fn setup(&mut self, builder: &mut RgPassBuilder) {
        if let Some(debug_image) = self.debug_image {
            builder.read_image(debug_image, RgImageState::SHADER_READ_FRAGMENT);
        }
        builder.read_write_image(self.canvas_color, RgImageState::COLOR_ATTACHMENT_READ_WRITE);
    }

    fn execute(&self, ctx: &RgPassContext<'_>) {
        if self.ui_draw_data.total_vtx_count == 0 {
            return;
        }

        let canvas_color_view_handle =
            ctx.get_image_view_handle(self.canvas_color).expect("GuiPass: canvas_color not found");
        let canvas_color_view = ctx.resource_manager.get_image_view(canvas_color_view_handle).unwrap();

        let frame_label = self.gpu_store.frame_counter.frame_label();
        self.gui_pass.draw(
            frame_label,
            &self.gpu_store.global_descriptor_sets,
            &self.gpu_store.bindless_manager,
            canvas_color_view.handle(),
            self.canvas_extent,
            ctx.cmd,
            self.gui_mesh,
            self.ui_draw_data,
            self.tex_map,
        );
    }
}
