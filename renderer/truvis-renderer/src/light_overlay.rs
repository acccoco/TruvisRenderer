use renderer_kit::camera::Camera;
use truvis_world::{LightTarget, SceneReadView};

use crate::overlay_geometry::OverlayGeometry;

/// 绘制和命中共用本帧投影及稳定排序，不持有 GPU 资源。
struct LightIcon {
    target: LightTarget,
    center: glam::Vec2,
    depth: f32,
}

#[derive(Default)]
pub(crate) struct LightOverlay {
    icons: Vec<LightIcon>,
    wire_lines: Vec<(glam::Vec2, glam::Vec2)>,
    wire_target: Option<LightTarget>,
    hovered: Option<LightTarget>,
    viewport: glam::Vec2,
}

impl LightIcon {
    fn draw(&self, geometry: &mut OverlayGeometry<'_>, color: glam::Vec4) {
        let c = self.center;
        match self.target {
            LightTarget::Point(_) => {
                for index in 0..16 {
                    let a = index as f32 * std::f32::consts::TAU / 16.0;
                    let b = (index + 1) as f32 * std::f32::consts::TAU / 16.0;
                    geometry.screen_line(
                        c + glam::vec2(a.cos(), a.sin()) * 5.0,
                        c + glam::vec2(b.cos(), b.sin()) * 5.0,
                        color,
                    );
                    if index % 2 == 0 {
                        geometry.screen_line(
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
                    geometry.screen_line(c + glam::Vec2::from(a), c + glam::Vec2::from(b), color);
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
                    geometry.screen_line(c + glam::Vec2::from(a), c + glam::Vec2::from(b), color);
                }
            }
        }
    }
}

/// update 中读取 World，生成只用于本帧的辅助线段。
struct LightWireBuilder<'a> {
    camera: &'a Camera,
    viewport: glam::Vec2,
    lines: &'a mut Vec<(glam::Vec2, glam::Vec2)>,
}

impl LightWireBuilder<'_> {
    const SEGMENTS: usize = 48;

    fn world_line(&mut self, a: glam::Vec3, b: glam::Vec3) {
        if let Some(line) = self.camera.project_segment_to_viewport(a, b, self.viewport) {
            self.lines.push(line);
        }
    }

    fn ring(&mut self, center: glam::Vec3, u: glam::Vec3, v: glam::Vec3) {
        for index in 0..Self::SEGMENTS {
            let a = index as f32 * std::f32::consts::TAU / Self::SEGMENTS as f32;
            let b = (index + 1) as f32 * std::f32::consts::TAU / Self::SEGMENTS as f32;
            self.world_line(center + u * a.cos() + v * a.sin(), center + u * b.cos() + v * b.sin());
        }
    }

    fn wire(&mut self, scene: SceneReadView<'_>, target: LightTarget) {
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
                    self.ring(position, u * 0.25, v * 0.25);
                }
            }
            LightTarget::Spot(handle) => {
                let light = &scene.spot_light_map()[handle];
                let Some(dir) = glam::Vec3::from(light.dir).try_normalize() else {
                    return;
                };
                let u = dir.any_orthonormal_vector();
                let v = dir.cross(u);
                self.world_line(position, position + dir);
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
                    self.ring(center, u * radius, v * radius);
                    for side in [u, -u, v, -v] {
                        self.world_line(position, center + side * radius);
                    }
                }
            }
            LightTarget::Area(handle) => {
                let light = &scene.area_light_map()[handle];
                let u = glam::Vec3::from(light.half_u);
                let v = glam::Vec3::from(light.half_v);
                let corners = [position - u - v, position + u - v, position + u + v, position - u + v];
                for index in 0..4 {
                    self.world_line(corners[index], corners[(index + 1) % 4]);
                }
                if let Some(normal) = u.cross(v).try_normalize() {
                    let tip = position + normal * 0.25;
                    let side = normal.any_orthonormal_vector() * 0.04;
                    self.world_line(position, tip);
                    self.world_line(tip, tip - normal * 0.06 + side);
                    self.world_line(tip, tip - normal * 0.06 - side);
                }
            }
        }
    }
}

impl LightOverlay {
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

    pub(crate) fn refresh_snapshot(
        &mut self,
        scene: SceneReadView<'_>,
        camera: &Camera,
        selected: Option<LightTarget>,
        mouse: Option<glam::Vec2>,
    ) {
        self.wire_lines.clear();
        self.wire_target = selected;
        self.hovered = mouse.and_then(|mouse| self.hit_test(mouse));
        if let Some(target) = selected {
            LightWireBuilder {
                camera,
                viewport: self.viewport,
                lines: &mut self.wire_lines,
            }
            .wire(scene, target);
        }
    }

    pub(crate) fn append_geometry(&self, geometry: &mut OverlayGeometry<'_>, selected: Option<LightTarget>) {
        if selected.is_some() && selected == self.wire_target {
            for &(a, b) in &self.wire_lines {
                geometry.screen_line(a, b, Self::SELECTED_COLOR.truncate().extend(0.45));
            }
        }
        for icon in &self.icons {
            let color = if Some(icon.target) == selected {
                Self::SELECTED_COLOR
            } else if Some(icon.target) == self.hovered {
                Self::HOVER_COLOR
            } else {
                Self::DEFAULT_COLOR
            };
            icon.draw(geometry, color);
        }
    }

    pub(crate) fn clear(&mut self) {
        self.icons.clear();
        self.wire_lines.clear();
        self.wire_target = None;
        self.hovered = None;
    }
}
