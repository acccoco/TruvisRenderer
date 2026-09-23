use glam::{EulerRot, Mat4, Quat, Vec3};

use crate::{SceneEditError, components::transform::TransformTrs};

/// 只保存本次编辑的字段；不把 Editor 的旧快照写回正在被 Gizmo 修改的灯光。
#[derive(Clone, Copy, Default)]
pub struct LightPatch {
    pub position: Option<Vec3>,
    pub radiance: Option<Vec3>,
    pub direction: Option<Vec3>,
    pub inner_angle: Option<f32>,
    pub outer_angle: Option<f32>,
    pub rotation_degrees: Option<Vec3>,
    pub width: Option<f32>,
    pub height: Option<f32>,
}

impl LightPatch {
    pub(crate) fn invalid(reason: &str) -> SceneEditError {
        SceneEditError::InvalidLightData { reason: reason.into() }
    }

    pub(crate) fn validate_common(position: Vec3, radiance: Vec3) -> Result<(), SceneEditError> {
        if !position.is_finite() || !radiance.is_finite() || radiance.min_element() < 0.0 {
            return Err(Self::invalid("position must be finite and radiance finite and nonnegative"));
        }
        Ok(())
    }

    pub(crate) fn has_spot_fields(&self) -> bool {
        self.direction.is_some() || self.inner_angle.is_some() || self.outer_angle.is_some()
    }

    pub(crate) fn has_area_fields(&self) -> bool {
        self.rotation_degrees.is_some() || self.width.is_some() || self.height.is_some()
    }
}

/// Area 的临时编辑投影；半轴始终是权威，不保存第二份 transform。
pub struct AreaLightShape {
    pub rotation_degrees: Vec3,
    pub width: f32,
    pub height: f32,
}

impl AreaLightShape {
    pub fn from_axes(u: Vec3, v: Vec3) -> Option<Self> {
        let normal = u.cross(v).try_normalize()?;
        let trs = TransformTrs::try_from_matrix(Mat4::from_cols(
            (u * 2.0).extend(0.0),
            (v * 2.0).extend(0.0),
            normal.extend(0.0),
            glam::Vec4::W,
        ))
        .ok()?;
        Some(Self {
            rotation_degrees: trs.rotation_euler_degrees(),
            width: trs.scale.x,
            height: trs.scale.y,
        })
    }

    pub(crate) fn matches_patch(&self, patch: LightPatch) -> bool {
        patch.rotation_degrees.is_none_or(|v| v == self.rotation_degrees) &&
            patch.width.is_none_or(|v| v == self.width) &&
            patch.height.is_none_or(|v| v == self.height)
    }

    pub(crate) fn apply(&mut self, patch: LightPatch) -> Result<(Vec3, Vec3), SceneEditError> {
        self.rotation_degrees = patch.rotation_degrees.unwrap_or(self.rotation_degrees);
        self.width = patch.width.unwrap_or(self.width);
        self.height = patch.height.unwrap_or(self.height);
        if !self.rotation_degrees.is_finite() ||
            !self.width.is_finite() ||
            !self.height.is_finite() ||
            self.width <= 0.0 ||
            self.height <= 0.0
        {
            return Err(LightPatch::invalid("area rotation must be finite and dimensions positive"));
        }
        let r = self.rotation_degrees * (std::f32::consts::PI / 180.0);
        let rotation = Quat::from_euler(EulerRot::XYZ, r.x, r.y, r.z);
        let u = rotation * Vec3::X * (self.width * 0.5);
        let v = rotation * Vec3::Y * (self.height * 0.5);
        if Self::from_axes(u, v).is_none() {
            return Err(LightPatch::invalid("area dimensions are degenerate or out of range"));
        }
        Ok((u, v))
    }
}
