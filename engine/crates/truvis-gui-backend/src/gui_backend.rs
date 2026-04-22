//! 参考 imgui-rs-vulkan-renderer

use std::collections::HashMap;

use ash::vk;
use imgui::{DrawData, FontAtlasTexture, TextureId};

use truvis_gfx::resources::image_view::GfxImageViewDesc;
use truvis_gfx::{basic::color::LabelColor, gfx::Gfx, resources::image::GfxImage};
use truvis_render_interface::bindless_manager::BindlessManager;
use truvis_render_interface::frame_counter::FrameCounter;
use truvis_render_interface::gfx_resource_manager::GfxResourceManager;
use truvis_render_interface::handles::GfxImageViewHandle;
use truvis_render_interface::pipeline_settings::FrameLabel;

use crate::gui_mesh::GuiMesh;

// TODO 这个东西和 GuiHost 的重复了
const FONT_TEXTURE_ID: usize = 0;

pub struct GuiBackend {
    /// 存放多帧 imgui 的 mesh 数据
    pub gui_meshes: [GuiMesh; FrameCounter::fif_count()],

    fonts_image_view_handle: Option<GfxImageViewHandle>,
    font_tex_id: TextureId,

    pub tex_map: HashMap<TextureId, GfxImageViewHandle>,
}
impl Default for GuiBackend {
    fn default() -> Self {
        Self::new()
    }
}

// new & init
impl GuiBackend {
    pub fn new() -> Self {
        let gui_meshes = FrameCounter::frame_labes().map(GuiMesh::new);

        Self {
            gui_meshes,
            fonts_image_view_handle: None,
            font_tex_id: TextureId::new(0),

            tex_map: Default::default(),
        }
    }

    pub fn register_font(
        &mut self,
        bindless_manager: &mut BindlessManager,
        gfx_resource_manager: &mut GfxResourceManager,
        font_atlas: FontAtlasTexture,
        font_tex_id: TextureId,
    ) {
        let fonts_image = GfxImage::from_rgba8(font_atlas.width, font_atlas.height, font_atlas.data, "imgui-fonts");
        let fonts_image_handle = gfx_resource_manager.register_image(fonts_image);
        let fonts_image_view_handle = gfx_resource_manager.get_or_create_image_view(
            fonts_image_handle,
            GfxImageViewDesc::new_2d(vk::Format::R8G8B8A8_UNORM, vk::ImageAspectFlags::COLOR),
            "imgui-fonts",
        );
        bindless_manager.register_srv(fonts_image_view_handle);

        self.fonts_image_view_handle = Some(fonts_image_view_handle);
        self.font_tex_id = font_tex_id;
    }
}
// tools
impl GuiBackend {
    // TODO 这个函数设计的非常别扭
    /// # Phase: Render
    ///
    /// 使用 imgui 将 ui 操作编译为 draw data；构建 draw 需要的 mesh 数据
    pub fn prepare_render_data(&mut self, draw_data: &DrawData, frame_label: FrameLabel) {
        Gfx::get().gfx_queue().begin_label("[ui-pass]create-mesh", LabelColor::COLOR_STAGE);
        {
            self.gui_meshes[*frame_label].grow_if_needed(draw_data);
            self.gui_meshes[*frame_label].fill_vertex_buffer(draw_data);
            self.gui_meshes[*frame_label].fill_index_buffer(draw_data);
        }
        Gfx::get().gfx_queue().end_label();

        self.tex_map = HashMap::from([(
            imgui::TextureId::new(FONT_TEXTURE_ID) as imgui::TextureId,
            self.fonts_image_view_handle.unwrap(),
        )]);
    }
}
