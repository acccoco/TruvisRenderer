//! Registrable overlay module system.

use ash::vk;

use truvis_render_interface::pipeline_settings::PipelineSettings;
use truvis_renderer::platform::camera::Camera;

/// Read-only snapshot + mutable pipeline settings passed to each overlay per frame.
pub struct OverlayContext<'a> {
    pub delta_time_s: f32,
    pub swapchain_extent: vk::Extent2D,
    pub camera: &'a Camera,
    pub accum_frames_num: usize,
    pub pipeline_settings: &'a mut PipelineSettings,
}

/// A registrable UI overlay module.
pub trait OverlayModule {
    fn build_ui(&mut self, ui: &imgui::Ui, ctx: &mut OverlayContext);
}

// ---------------------------------------------------------------------------
// Default overlays
// ---------------------------------------------------------------------------

pub struct DebugInfoOverlay;

impl OverlayModule for DebugInfoOverlay {
    fn build_ui(&mut self, ui: &imgui::Ui, ctx: &mut OverlayContext) {
        ui.window("##overlay")
            .position([0.0, 0.0], imgui::Condition::Always)
            .size([ctx.swapchain_extent.width as f32, ctx.swapchain_extent.height as f32], imgui::Condition::Always)
            .flags(
                imgui::WindowFlags::NO_TITLE_BAR
                    | imgui::WindowFlags::NO_RESIZE
                    | imgui::WindowFlags::NO_MOVE
                    | imgui::WindowFlags::NO_SCROLLBAR
                    | imgui::WindowFlags::NO_SCROLL_WITH_MOUSE
                    | imgui::WindowFlags::NO_COLLAPSE
                    | imgui::WindowFlags::NO_BACKGROUND
                    | imgui::WindowFlags::NO_SAVED_SETTINGS
                    | imgui::WindowFlags::NO_MOUSE_INPUTS
                    | imgui::WindowFlags::NO_FOCUS_ON_APPEARING
                    | imgui::WindowFlags::NO_BRING_TO_FRONT_ON_FOCUS
                    | imgui::WindowFlags::NO_NAV_INPUTS
                    | imgui::WindowFlags::NO_NAV_FOCUS,
            )
            .build(|| {
                ui.set_cursor_pos([5.0, 5.0]);
                ui.text(format!("FPS: {:.2}", 1.0 / ctx.delta_time_s));
                ui.text(format!("swapchain: {:.0}x{:.0}", ctx.swapchain_extent.width, ctx.swapchain_extent.height));

                let camera = ctx.camera;
                ui.text(format!(
                    "CameraPos: ({:.2}, {:.2}, {:.2})",
                    camera.position.x, camera.position.y, camera.position.z
                ));
                ui.text(format!(
                    "CameraEuler: ({:.2}, {:.2}, {:.2})",
                    camera.euler_yaw_deg, camera.euler_pitch_deg, camera.euler_roll_deg
                ));
                ui.text(format!(
                    "CameraForward: ({:.2}, {:.2}, {:.2})",
                    camera.camera_forward().x,
                    camera.camera_forward().y,
                    camera.camera_forward().z
                ));
                ui.text(format!("CameraAspect: {:.2}", camera.asp));
                ui.text(format!("CameraFov(Vertical): {:.2}\u{00b0}", camera.fov_deg_vertical));
                ui.text(format!("Accum Frames: {}", ctx.accum_frames_num));
                ui.new_line();
            });
    }
}

pub struct PipelineControlsOverlay;

impl OverlayModule for PipelineControlsOverlay {
    fn build_ui(&mut self, ui: &imgui::Ui, ctx: &mut OverlayContext) {
        ui.window("Controls")
            .position([10.0, 200.0], imgui::Condition::FirstUseEver)
            .size([250.0, 200.0], imgui::Condition::FirstUseEver)
            .build(|| {
                let ps = &mut *ctx.pipeline_settings;
                ui.slider("channel", 0, 9, &mut ps.channel);
                ui.text(match ps.channel {
                    0 => "final",
                    1 => "normal",
                    2 => "base color",
                    3 => "not accum",
                    4 => "from NEE HDRI",
                    5 => "from emission",
                    6 => "from BDRF HDRi",
                    7 => "NEE bounce 0",
                    8 => "NEE bounce 1",
                    9 => "Irradiance Cache",
                    _ => "Unknown",
                });

                ui.separator();
                ui.text("Irradiance Cache");
                ui.checkbox("Enable IC", &mut ps.ic_enabled);

                ui.separator();
                ui.text("Denoise Settings");

                let denoise = &mut ps.denoise;
                ui.checkbox("Enable Denoise", &mut denoise.enabled);

                let _disabled = ui.begin_disabled(!denoise.enabled);
                ui.slider("Sigma Color", 0.01, 1.0, &mut denoise.sigma_color);
                ui.slider("Sigma Depth", 0.01, 2.0, &mut denoise.sigma_depth);
                ui.slider("Sigma Normal", 0.01, 2.0, &mut denoise.sigma_normal);
                ui.slider("Kernel Radius", 1, 5, &mut denoise.kernel_radius);
            });
    }
}

/// Returns the default set of overlay modules.
pub fn default_overlays() -> Vec<Box<dyn OverlayModule>> {
    vec![Box::new(DebugInfoOverlay), Box::new(PipelineControlsOverlay)]
}
