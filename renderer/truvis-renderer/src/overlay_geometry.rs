use renderer_render_passes::effects::viewport_overlay::ViewportOverlayVertex;
use truvis_render_foundation::render_view::RenderView;

/// 本帧屏幕几何构造器。所有输出共用物理像素、矩形裁剪和 clip-space ABI。
pub(crate) struct OverlayGeometry<'a> {
    viewport: glam::Vec2,
    clip_min: glam::Vec2,
    clip_max: glam::Vec2,
    vertices: &'a mut Vec<ViewportOverlayVertex>,
}

impl<'a> OverlayGeometry<'a> {
    pub(crate) const AXIS_COLORS: [glam::Vec4; 3] = [
        glam::vec4(0.95, 0.12, 0.10, 0.96),
        glam::vec4(0.18, 0.86, 0.22, 0.96),
        glam::vec4(0.20, 0.44, 1.00, 0.96),
    ];

    pub(crate) fn new(viewport: glam::Vec2, vertices: &'a mut Vec<ViewportOverlayVertex>) -> Self {
        Self {
            viewport,
            clip_min: glam::Vec2::ZERO,
            clip_max: viewport,
            vertices,
        }
    }

    /// CPU 矩形裁剪也用于右下角小视口，不增加额外 scissor 批次或 GPU pass。
    fn triangle(&mut self, triangle: [glam::Vec2; 3], color: glam::Vec4) {
        if !self.viewport.is_finite()
            || self.viewport.min_element() <= 0.0
            || !color.is_finite()
            || triangle.iter().any(|p| !p.is_finite())
        {
            return;
        }
        let mut points = [glam::Vec2::ZERO; 8];
        points[..3].copy_from_slice(&triangle);
        let mut count = 3;
        for (axis, boundary, sign) in [
            (0, self.clip_min.x, 1.0),
            (0, self.clip_max.x, -1.0),
            (1, self.clip_min.y, 1.0),
            (1, self.clip_max.y, -1.0),
        ] {
            if count == 0 {
                return;
            }
            let mut clipped = [glam::Vec2::ZERO; 8];
            let mut next_count = 0;
            let mut previous = points[count - 1];
            let mut previous_distance = (previous[axis] - boundary) * sign;
            for &current in &points[..count] {
                let distance = (current[axis] - boundary) * sign;
                if (distance >= 0.0) != (previous_distance >= 0.0) {
                    let mut intersection = previous.lerp(current, previous_distance / (previous_distance - distance));
                    intersection[axis] = boundary;
                    clipped[next_count] = intersection;
                    next_count += 1;
                }
                if distance >= 0.0 {
                    clipped[next_count] = current;
                    next_count += 1;
                }
                previous = current;
                previous_distance = distance;
            }
            points = clipped;
            count = next_count;
        }
        if count < 3 || points[..count].iter().any(|p| !p.is_finite()) {
            return;
        }
        for index in 1..count - 1 {
            let area = (points[index] - points[0]).perp_dot(points[index + 1] - points[0]);
            if !area.is_finite() || area.abs() <= f32::EPSILON {
                continue;
            }
            for p in [points[0], points[index], points[index + 1]] {
                let ndc = p / self.viewport;
                self.vertices.push(ViewportOverlayVertex {
                    position: glam::vec4(ndc.x * 2.0 - 1.0, 1.0 - ndc.y * 2.0, 0.0, 1.0).into(),
                    color: color.into(),
                });
            }
        }
    }

    pub(crate) fn screen_line(&mut self, a: glam::Vec2, b: glam::Vec2, color: glam::Vec4) {
        self.line(a, b, 2.0, color);
    }

    pub(crate) fn line(&mut self, a: glam::Vec2, b: glam::Vec2, width: f32, color: glam::Vec4) {
        let direction = b - a;
        let Some(normal) = glam::vec2(-direction.y, direction.x).try_normalize() else {
            return;
        };
        let side = normal * width * 0.5;
        self.triangle([a - side, b - side, b + side], color);
        self.triangle([b + side, a + side, a - side], color);
    }

    pub(crate) fn arrow(
        &mut self,
        a: glam::Vec2,
        b: glam::Vec2,
        shaft_width: f32,
        head_width: f32,
        head_length: f32,
        color: glam::Vec4,
    ) {
        let Some(direction) = (b - a).try_normalize() else {
            return;
        };
        let base = b - direction * head_length.min(a.distance(b));
        let side = glam::vec2(-direction.y, direction.x) * head_width * 0.5;
        self.line(a, base, shaft_width, color);
        self.triangle([b, base + side, base - side], color);
    }

    pub(crate) fn coordinate_gizmo(&mut self, view: &RenderView) {
        let size = 112.0_f32.min(self.viewport.min_element());
        if size <= 0.0 {
            return;
        }
        let margin = 24.0_f32.min((size / 4.0).floor());
        let origin = (self.viewport - glam::Vec2::splat(size + margin)).max(glam::Vec2::ZERO);
        self.clip_min = origin;
        self.clip_max = (origin + glam::Vec2::splat(size)).min(self.viewport);
        let center = origin + glam::Vec2::splat(size * 0.5);
        let fallback = [glam::Vec2::X, glam::Vec2::Y, glam::vec2(-0.7, -0.7).normalize()];
        for (index, axis) in [glam::Vec3::X, glam::Vec3::Y, glam::Vec3::Z].into_iter().enumerate() {
            let projected = view.view.transform_vector3(axis).truncate();
            let length = projected.length();
            let direction = if length > 0.001 { projected / length } else { fallback[index] };
            let axis_length = 0.46 + (0.74 - 0.46) * length.clamp(0.0, 1.0);
            let tip = center + direction * glam::vec2(1.0, -1.0) * axis_length * size * 0.5;
            self.arrow(center, tip, size * 0.030, size * 0.095, size * 0.080, Self::AXIS_COLORS[index]);
        }
        self.clip_min = glam::Vec2::ZERO;
        self.clip_max = self.viewport;
    }
}
