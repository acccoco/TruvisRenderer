use ash::vk;

use renderer_kit::camera::Camera;

pub struct FrameStatsOverlayData<'a> {
    pub camera: &'a Camera,
    pub swapchain_extent: vk::Extent2D,
    pub accum_frames_num: usize,
}

#[derive(Default)]
pub struct DebugInfoOverlay;

impl DebugInfoOverlay {
    pub fn build_overlay_ui(
        &mut self,
        ui: &imgui::Ui,
        camera: &Camera,
        swapchain_extent: vk::Extent2D,
        accum_frames_num: usize,
    ) {
        let stats = FrameStatsOverlayData {
            camera,
            swapchain_extent,
            accum_frames_num,
        };
        Self::build_frame_stats_hud(ui, &stats);
    }

    pub fn build_frame_stats_hud(ui: &imgui::Ui, stats: &FrameStatsOverlayData<'_>) {
        ui.window("##overlay")
            .position([0.0, 0.0], imgui::Condition::Always)
            .size(
                [
                    stats.swapchain_extent.width as f32,
                    stats.swapchain_extent.height as f32,
                ],
                imgui::Condition::Always,
            )
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
                Self::build_frame_stats_section(ui, stats);
            });
    }

    pub fn build_frame_stats_section(ui: &imgui::Ui, stats: &FrameStatsOverlayData<'_>) {
        let camera = stats.camera;
        ui.text(format!("FPS: {:.2}", ui.io().framerate));
        ui.text(format!("swapchain: {:.0}x{:.0}", stats.swapchain_extent.width, stats.swapchain_extent.height));
        ui.text(format!("CameraPos: ({:.2}, {:.2}, {:.2})", camera.position.x, camera.position.y, camera.position.z));
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
        ui.text(format!("Accum Frames: {}", stats.accum_frames_num));
        ui.new_line();
    }
}
