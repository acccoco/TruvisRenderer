use crate::platform::input_event::{ElementState, InputEvent, MouseButton};
use imgui::{DrawData, FontAtlasTexture, TextureId};
use truvis_path::TruvisPath;

const FONT_TEXTURE_ID: usize = 0;
const RENDER_IMAGE_ID: usize = 1;

pub struct GuiHost {
    pub imgui_ctx: imgui::Context,
    pub hidpi_factor: f64,
}
// new & init
impl Default for GuiHost {
    fn default() -> Self {
        Self::new()
    }
}

impl GuiHost {
    pub fn new() -> Self {
        let mut imgui_ctx = imgui::Context::create();
        // disable automatic saving .ini file
        imgui_ctx.set_ini_filename(None);

        // theme
        {
            let style = imgui_ctx.style_mut();
            style.use_dark_colors();
            // WindowBg: 半透明深色背景
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
        // io.font_global_scale = (1.0 / hidpi_factor) as f32;
        io.font_global_scale = 1.0;
        io.config_flags |= imgui::ConfigFlags::DOCKING_ENABLE;

        let fonts = self.imgui_ctx.fonts();
        let atlas_texture = fonts.build_rgba32_texture();

        (atlas_texture, imgui::TextureId::new(FONT_TEXTURE_ID))
    }
}
// update
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

    pub fn new_frame_dock(
        &mut self,
        duration: std::time::Duration,
        ui_build_func_main: impl FnOnce(&imgui::Ui, [f32; 2]),
        ui_build_func_right: impl FnOnce(&imgui::Ui),
    ) {
        self.imgui_ctx.io_mut().update_delta_time(duration);
        let ui = self.imgui_ctx.new_frame();

        unsafe {
            let viewport = imgui::sys::igGetMainViewport();
            let viewport_size = (*viewport).Size;
            let root_node_id = imgui::sys::igGetID_Str(c"MainDockSpace".as_ptr());

            ui.window("main dock space")
                .position([0.0, 0.0], imgui::Condition::Always)
                .size([viewport_size.x, viewport_size.y], imgui::Condition::Always)
                .flags(
                    imgui::WindowFlags::NO_MOVE
                        | imgui::WindowFlags::NO_TITLE_BAR
                        // | imgui::WindowFlags::MENU_BAR
                        | imgui::WindowFlags::NO_COLLAPSE
                        | imgui::WindowFlags::NO_BRING_TO_FRONT_ON_FOCUS
                        | imgui::WindowFlags::NO_NAV_FOCUS
                        | imgui::WindowFlags::NO_DOCKING
                        | imgui::WindowFlags::NO_BACKGROUND
                        | imgui::WindowFlags::NO_RESIZE,
                )
                .build(|| {
                    if imgui::sys::igDockBuilderGetNode(root_node_id).is_null() {
                        imgui::sys::igDockBuilderRemoveNode(root_node_id);
                        imgui::sys::igDockBuilderAddNode(root_node_id, imgui::sys::ImGuiDockNodeFlags_NoCloseButton);
                        imgui::sys::igDockBuilderSetNodeSize(root_node_id, (*imgui::sys::igGetMainViewport()).Size);
                        imgui::sys::igDockBuilderSetNodePos(root_node_id, imgui::sys::ImVec2 { x: 0.0, y: 0.0 });

                        // 首先将整个窗口分为左右两部分
                        let mut dock_main_id = root_node_id;
                        let dock_right_id = imgui::sys::igDockBuilderSplitNode(
                            dock_main_id,
                            imgui::sys::ImGuiDir_Right,
                            0.3,
                            std::ptr::null_mut(),
                            std::ptr::from_mut(&mut dock_main_id),
                        );

                        // 将左边部分再分为左右两部分
                        // let dock_left_id = imgui::sys::igDockBuilderSplitNode(
                        //     dock_main_id,
                        //     imgui::sys::ImGuiDir_Left,
                        //     0.2,
                        //     std::ptr::null_mut(),
                        //     std::ptr::from_mut(&mut dock_main_id),
                        // );

                        // 将中间部分在分为上下两部分
                        // let dock_down_id = imgui::sys::igDockBuilderSplitNode(
                        //     dock_main_id,
                        //     imgui::sys::ImGuiDir_Down,
                        //     0.2,
                        //     std::ptr::null_mut(),
                        //     std::ptr::from_mut(&mut dock_main_id),
                        // );

                        // 隐藏中央节点的 Tab
                        // let center_node = imgui::sys::igDockBuilderGetNode(dock_main_id);
                        // (*center_node).LocalFlags |= imgui::sys::ImGuiDockNodeFlags_HiddenTabBar;

                        log::info!("main node id: {}", dock_main_id);
                        imgui::sys::igDockBuilderDockWindow(c"right".as_ptr(), dock_right_id);
                        imgui::sys::igDockBuilderDockWindow(c"render".as_ptr(), dock_main_id);
                        imgui::sys::igDockBuilderFinish(root_node_id);
                    }

                    imgui::sys::igDockSpace(
                        root_node_id,
                        imgui::sys::ImVec2 { x: 0.0, y: 0.0 },
                        imgui::sys::ImGuiDockNodeFlags_None as _,
                        std::ptr::null(),
                    );
                });

            // 中间的窗口，用于放置渲染内容
            ui.window("render")
                .title_bar(true)
                .menu_bar(false)
                // .resizable(false)
                // .bg_alpha(0.0)
                .draw_background(false)
                .build(|| {
                    let _window_pos = ui.window_pos();
                    let window_region_max = ui.window_content_region_max();
                    let window_region_min = ui.window_content_region_min();
                    let window_size = [
                        window_region_max[0] - window_region_min[0],
                        window_region_max[1] - window_region_min[1],
                    ];
                    // let hidpi_factor = self.platform.hidpi_factor() as f32;
                    let _hidpi_factor = 1.0;

                    // self.render_region.offset = vk::Offset2D {
                    //     x: (window_pos[0] * hidpi_factor) as i32,
                    //     y: (window_pos[1] * hidpi_factor) as i32,
                    // };
                    // self.render_region.extent = vk::Extent2D {
                    //     width: (window_size[0] * hidpi_factor) as u32,
                    //     height: (window_size[1] * hidpi_factor) as u32,
                    // };

                    imgui::Image::new(imgui::TextureId::new(RENDER_IMAGE_ID), [window_size[0], window_size[1]])
                        .build(ui);

                    ui_build_func_main(ui, window_size);
                });

            // 右侧的窗口，用于放置各种设置
            ui.window("right").draw_background(false).build(|| {
                ui.text("test window.");
                let root_node = imgui::sys::igDockBuilderGetNode(root_node_id);
                let root_pos = (*root_node).Pos;
                let root_size = (*root_node).Size;
                ui.text(format!("Root Node Position: ({:.1},{:.1})", root_pos.x, root_pos.y));
                ui.text(format!("Root Node Size: ({:.1},{:.1})", root_size.x, root_size.y));

                ui.text(format!("Hidpi Factor: {}", self.hidpi_factor));
                // ui.text(format!("Window Size: ({:?})", self.render_region.extent));
                // ui.text(format!("Window Position: ({:?})", self.render_region.offset));
                ui.new_line();

                ui_build_func_right(ui);
            });
        }
    }

    pub fn compile_ui(&mut self) {
        self.imgui_ctx.render();
    }

    /// 确保之前调用过 compile_ui
    pub fn get_render_data(&self) -> &DrawData {
        unsafe { &*(imgui::sys::igGetDrawData() as *mut DrawData) }
    }
}
