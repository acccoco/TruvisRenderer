use ash::vk;

use renderer_kit::{camera::Camera, subsystem::SubsystemLifecycle};
use renderer_render_passes::effects::light_overlay::{
    LightOverlayPass, LightOverlayRgPass, LightOverlayVertex, LightOverlayVertexLayout,
};
use truvis_gfx::{
    gfx::GfxResourceCtx,
    resources::{lifecycle::DestroyReason, special_buffers::vertex_buffer::GfxVertexBuffer},
};
use truvis_render_foundation::frame_label::FrameLabel;
use truvis_render_graph::render_graph::{RenderGraphBuilder, RgImageHandle};
use truvis_render_runtime::render_runtime::{
    RenderRuntimeInitCtx, RenderRuntimeRenderCtx, RenderRuntimeResizeCtx, RenderRuntimeShutdownCtx,
};
use truvis_world::{LightTarget, SceneReadView};

/// 本帧图标的唯一投影，绘制和 CPU 命中共用矩形与稳定排序。
struct LightIcon {
    target: LightTarget,
    center: glam::Vec2,
    depth: f32,
}

#[derive(Default)]
pub(crate) struct LightOverlaySubsystem {
    resources: Option<LightOverlayResources>,
    icons: Vec<LightIcon>,
    icons_vertices: Vec<LightOverlayVertex>,
    wire_vertices: Vec<LightOverlayVertex>,
    wire_target: Option<LightTarget>,
    viewport: glam::Vec2,
    vertex_count: u32,
}

struct LightOverlayResources {
    pass: LightOverlayPass,
    format: vk::Format,
    /// 当前 frame label 已由 Runtime 等待；只写或扩容该副本，不改仍在飞行中的 buffer。
    buffers: [GfxVertexBuffer<LightOverlayVertexLayout>; FrameLabel::COUNT],
}

/// 局部几何构造器，不持有 scene 身份、输入或 GPU 资源。
struct OverlayGeometry<'a> {
    camera: &'a Camera,
    viewport: glam::Vec2,
    vertices: &'a mut Vec<LightOverlayVertex>,
}

impl OverlayGeometry<'_> {
    const LINE_WIDTH: f32 = 2.0;
    const SEGMENTS: usize = 48;

    fn screen_line(&mut self, a: glam::Vec2, b: glam::Vec2, color: glam::Vec4) {
        let direction = b - a;
        if !a.is_finite() || !b.is_finite() || direction.length_squared() < 1e-8 {
            return;
        }
        let Some(normal) = glam::vec2(-direction.y, direction.x).try_normalize() else {
            return;
        };
        let side = normal * (Self::LINE_WIDTH * 0.5);
        for position in [a - side, b - side, b + side, b + side, a + side, a - side] {
            let ndc = position / self.viewport;
            self.vertices.push(LightOverlayVertex {
                position: glam::vec4(ndc.x * 2.0 - 1.0, 1.0 - ndc.y * 2.0, 0.0, 1.0).into(),
                color: color.into(),
            });
        }
    }

    fn world_line(&mut self, mut a: glam::Vec3, mut b: glam::Vec3, color: glam::Vec4) {
        if !a.is_finite() || !b.is_finite() {
            return;
        }
        let view = self.camera.get_view_matrix();
        let da = -view.transform_point3(a).z;
        let db = -view.transform_point3(b).z;
        let near = self.camera.near * 1.001;
        if da < near && db < near {
            return;
        }
        if da < near {
            a = a.lerp(b, (near - da) / (db - da));
        } else if db < near {
            b = a.lerp(b, (near - da) / (db - da));
        }
        if let (Some(a), Some(b)) =
            (self.camera.project_to_viewport(a, self.viewport), self.camera.project_to_viewport(b, self.viewport))
        {
            self.screen_line(a, b, color);
        }
    }

    fn ring(&mut self, center: glam::Vec3, u: glam::Vec3, v: glam::Vec3, color: glam::Vec4) {
        for index in 0..Self::SEGMENTS {
            let a = index as f32 * std::f32::consts::TAU / Self::SEGMENTS as f32;
            let b = (index + 1) as f32 * std::f32::consts::TAU / Self::SEGMENTS as f32;
            self.world_line(center + u * a.cos() + v * a.sin(), center + u * b.cos() + v * b.sin(), color);
        }
    }

    fn icon(&mut self, icon: &LightIcon, color: glam::Vec4) {
        let c = icon.center;
        match icon.target {
            LightTarget::Point(_) => {
                for index in 0..16 {
                    let a = index as f32 * std::f32::consts::TAU / 16.0;
                    let b = (index + 1) as f32 * std::f32::consts::TAU / 16.0;
                    self.screen_line(
                        c + glam::vec2(a.cos(), a.sin()) * 5.0,
                        c + glam::vec2(b.cos(), b.sin()) * 5.0,
                        color,
                    );
                    if index % 2 == 0 {
                        self.screen_line(
                            c + glam::vec2(a.cos(), a.sin()) * 8.0,
                            c + glam::vec2(a.cos(), a.sin()) * 11.0,
                            color,
                        );
                    }
                }
            }
            LightTarget::Spot(_) => {
                for (a, b) in [
                    ([-5., -8.], [5., -8.]),
                    ([5., -8.], [5., -2.]),
                    ([5., -2.], [-5., -2.]),
                    ([-5., -2.], [-5., -8.]),
                    ([-4., 1.], [-10., 10.]),
                    ([4., 1.], [10., 10.]),
                    ([-10., 10.], [10., 10.]),
                    ([0., 1.], [0., 7.]),
                ] {
                    self.screen_line(c + glam::Vec2::from(a), c + glam::Vec2::from(b), color);
                }
            }
            LightTarget::Area(_) => {
                for (a, b) in [
                    ([-10., -8.], [10., -8.]),
                    ([10., -8.], [10., 5.]),
                    ([10., 5.], [-10., 5.]),
                    ([-10., 5.], [-10., -8.]),
                    ([-6., 8.], [-6., 11.]),
                    ([0., 8.], [0., 11.]),
                    ([6., 8.], [6., 11.]),
                ] {
                    self.screen_line(c + glam::Vec2::from(a), c + glam::Vec2::from(b), color);
                }
            }
        }
    }

    fn wire(&mut self, scene: SceneReadView<'_>, target: LightTarget, color: glam::Vec4) {
        let Some(position) = scene.light_position(target) else {
            return;
        };
        match target {
            LightTarget::Point(_) => {
                for (u, v) in [
                    (glam::Vec3::X, glam::Vec3::Y),
                    (glam::Vec3::Y, glam::Vec3::Z),
                    (glam::Vec3::Z, glam::Vec3::X),
                ] {
                    self.ring(position, u * 0.25, v * 0.25, color);
                }
            }
            LightTarget::Spot(handle) => {
                let light = &scene.spot_light_map()[handle];
                let Some(dir) = glam::Vec3::from(light.dir).try_normalize() else {
                    return;
                };
                let u = dir.any_orthonormal_vector();
                let v = dir.cross(u);
                self.world_line(position, position + dir, color);
                let mut angles = [light.inner_angle, light.outer_angle];
                // 先保留原始值再排序，避免 min/max 将 NaN 吞并为另一个有效角度。
                angles.sort_by(f32::total_cmp);
                for angle in angles {
                    if !angle.is_finite() || !(0.0..=std::f32::consts::PI).contains(&angle) {
                        continue;
                    }
                    // 长度只用于辅助显示，不表示 shader 存在有限照射距离。
                    let center = position + dir * angle.cos();
                    let radius = angle.sin();
                    self.ring(center, u * radius, v * radius, color);
                    for side in [u, -u, v, -v] {
                        self.world_line(position, center + side * radius, color);
                    }
                }
            }
            LightTarget::Area(handle) => {
                let light = &scene.area_light_map()[handle];
                let u = glam::Vec3::from(light.half_u);
                let v = glam::Vec3::from(light.half_v);
                let corners = [position - u - v, position + u - v, position + u + v, position - u + v];
                for index in 0..4 {
                    self.world_line(corners[index], corners[(index + 1) % 4], color);
                }
                if let Some(normal) = u.cross(v).try_normalize() {
                    let tip = position + normal * 0.25;
                    let side = normal.any_orthonormal_vector() * 0.04;
                    self.world_line(position, tip, color);
                    self.world_line(tip, tip - normal * 0.06 + side, color);
                    self.world_line(tip, tip - normal * 0.06 - side, color);
                }
            }
        }
    }
}

impl LightOverlaySubsystem {
    const DEFAULT_COLOR: glam::Vec4 = glam::vec4(1.0, 0.84, 0.35, 1.0);
    const HOVER_COLOR: glam::Vec4 = glam::vec4(1.0, 1.0, 0.8, 1.0);
    const SELECTED_COLOR: glam::Vec4 = glam::vec4(0.2, 0.65, 1.0, 1.0);

    pub(crate) fn project(&mut self, scene: SceneReadView<'_>, camera: &Camera, viewport: glam::Vec2) {
        self.viewport = viewport;
        self.icons.clear();
        let targets = scene
            .point_light_map()
            .keys()
            .map(LightTarget::Point)
            .chain(scene.spot_light_map().keys().map(LightTarget::Spot))
            .chain(scene.area_light_map().keys().map(LightTarget::Area));
        for target in targets {
            let Some(position) = scene.light_position(target) else {
                continue;
            };
            let Some(center) = camera.project_to_viewport(position, viewport) else {
                continue;
            };
            if center.x + 12.0 <= 0.0
                || center.y + 12.0 <= 0.0
                || center.x - 12.0 >= viewport.x
                || center.y - 12.0 >= viewport.y
            {
                continue;
            }
            self.icons.push(LightIcon {
                target,
                center,
                depth: -camera.get_view_matrix().transform_point3(position).z,
            });
        }
        self.icons.sort_by(|a, b| b.depth.total_cmp(&a.depth).then_with(|| a.target.cmp(&b.target)));
    }

    pub(crate) fn hit_test(&self, mouse: glam::Vec2) -> Option<LightTarget> {
        if !mouse.is_finite() || mouse.min_element() < 0.0 || mouse.x >= self.viewport.x || mouse.y >= self.viewport.y {
            return None;
        }
        self.icons.iter().rev().find(|icon| (mouse - icon.center).abs().max_element() <= 14.0).map(|icon| icon.target)
    }

    pub(crate) fn refresh_geometry(
        &mut self,
        scene: SceneReadView<'_>,
        camera: &Camera,
        selected: Option<LightTarget>,
        mouse: Option<glam::Vec2>,
    ) {
        self.icons_vertices.clear();
        self.wire_vertices.clear();
        self.wire_target = selected;
        if self.viewport.min_element() <= 0.0 {
            return;
        }
        let hovered = mouse.and_then(|mouse| self.hit_test(mouse));
        let mut geometry = OverlayGeometry {
            camera,
            viewport: self.viewport,
            vertices: &mut self.icons_vertices,
        };
        for icon in &self.icons {
            let color = if Some(icon.target) == selected {
                Self::SELECTED_COLOR
            } else if Some(icon.target) == hovered {
                Self::HOVER_COLOR
            } else {
                Self::DEFAULT_COLOR
            };
            geometry.icon(icon, color);
        }
        if let Some(target) = selected {
            let mut geometry = OverlayGeometry {
                camera,
                viewport: self.viewport,
                vertices: &mut self.wire_vertices,
            };
            geometry.wire(scene, target, Self::SELECTED_COLOR.truncate().extend(0.45));
        }
    }

    pub(crate) fn prepare_render(&mut self, ctx: &RenderRuntimeRenderCtx<'_>, selected: Option<LightTarget>) {
        let Some(resources) = self.resources.as_mut() else {
            return;
        };
        // after_prepare 可能已选中网格；旧灯光 frame 只能贡献普通图标。
        if selected != self.wire_target {
            self.wire_vertices.clear();
            for vertex in &mut self.icons_vertices {
                vertex.color = Self::DEFAULT_COLOR.into();
            }
        }
        self.wire_vertices.extend_from_slice(&self.icons_vertices);
        self.vertex_count = self.wire_vertices.len() as u32;
        if self.vertex_count == 0 {
            return;
        }
        let label = ctx.record_ctx.frame_timing.frame_label();
        let buffer = &mut resources.buffers[*label];
        if buffer.vertex_cnt() < self.wire_vertices.len() {
            buffer.destroy_mut(ctx.resource_ctx, DestroyReason::ImmediateRelease);
            *buffer = Self::new_buffer(ctx.resource_ctx, label, self.wire_vertices.len().next_power_of_two());
        }
        buffer.transfer_data_by_mmap(ctx.resource_ctx, &self.wire_vertices);
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
            "light-overlay",
            LightOverlayRgPass {
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
    ) -> GfxVertexBuffer<LightOverlayVertexLayout> {
        GfxVertexBuffer::new(ctx, capacity, true, format!("light-overlay-{label}"))
    }
}

impl SubsystemLifecycle for LightOverlaySubsystem {
    fn init(&mut self, ctx: &mut RenderRuntimeInitCtx<'_>) {
        let format = ctx.present.swapchain_image_info().image_format;
        self.resources = Some(LightOverlayResources {
            pass: LightOverlayPass::new(ctx.device_ctx, format),
            format,
            buffers: FrameLabel::ALL.map(|label| Self::new_buffer(ctx.resource_ctx, label, 2048)),
        });
    }
    fn on_resize(&mut self, ctx: &mut RenderRuntimeResizeCtx<'_>) {
        self.icons.clear();
        self.icons_vertices.clear();
        self.wire_vertices.clear();
        self.vertex_count = 0;
        if let Some(resources) = self.resources.as_mut() {
            let format = ctx.present.swapchain_image_info().image_format;
            if resources.format != format {
                let old = std::mem::replace(&mut resources.pass, LightOverlayPass::new(ctx.device_ctx, format));
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
