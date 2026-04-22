use imgui::{DrawData, FontAtlasTexture, TextureId};

use truvis_app_api::input_event::{ElementState, InputEvent, MouseButton};
use truvis_path::TruvisPath;

const FONT_TEXTURE_ID: usize = 0;

pub struct GuiHost {
    pub imgui_ctx: imgui::Context,
    pub hidpi_factor: f64,
}

impl Default for GuiHost {
    fn default() -> Self {
        Self::new()
    }
}

impl GuiHost {
    pub fn new() -> Self {
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
            hidpi_factor: 1.0,
        }
    }

    pub fn init_font(&mut self) -> (FontAtlasTexture<'_>, TextureId) {
        let hidpi_factor = self.hidpi_factor;
        let font_size = (13.0 * hidpi_factor) as f32;

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

        let font_texture_id = imgui::TextureId::from(0);
        self.imgui_ctx.fonts().tex_id = font_texture_id;

        let io = self.imgui_ctx.io_mut();
        io.font_global_scale = 1.0;
        io.config_flags |= imgui::ConfigFlags::DOCKING_ENABLE;

        let fonts = self.imgui_ctx.fonts();
        let atlas_texture = fonts.build_rgba32_texture();

        (atlas_texture, imgui::TextureId::new(FONT_TEXTURE_ID))
    }
}

impl GuiHost {
    pub fn handle_event(&mut self, event: &InputEvent) {
        let io = self.imgui_ctx.io_mut();
        match event {
            InputEvent::Resized {
                physical_width,
                physical_height,
            } => {
                io.display_size = [*physical_width as f32, *physical_height as f32];
            }
            InputEvent::MouseMoved { physical_position } => {
                io.add_mouse_pos_event([physical_position[0] as f32, physical_position[1] as f32]);
            }
            InputEvent::MouseButtonInput { button, state } => {
                if let Some(mb) = match button {
                    MouseButton::Left => Some(imgui::MouseButton::Left),
                    MouseButton::Right => Some(imgui::MouseButton::Right),
                    MouseButton::Middle => Some(imgui::MouseButton::Middle),
                    _ => None,
                } {
                    let pressed = *state == ElementState::Pressed;
                    io.add_mouse_button_event(mb, pressed);
                }
            }
            _ => {}
        }
    }

    pub fn new_frame(&mut self, duration: std::time::Duration, ui_func: impl FnOnce(&imgui::Ui)) {
        self.imgui_ctx.io_mut().update_delta_time(duration);
        let ui = self.imgui_ctx.new_frame();
        ui_func(ui);
    }

    pub fn compile_ui(&mut self) {
        self.imgui_ctx.render();
    }

    pub fn get_render_data(&self) -> &DrawData {
        unsafe { &*(imgui::sys::igGetDrawData() as *mut DrawData) }
    }
}
