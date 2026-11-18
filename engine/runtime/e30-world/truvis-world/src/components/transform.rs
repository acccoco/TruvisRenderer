use glam::{EulerRot, Mat4, Quat, Vec3};

/// CPU world 矩阵的临时 TRS 分解，不作为与 Matrix 并存的可变状态。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TransformTrs {
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

/// 分解失败不改变原矩阵；调用方应保留其他 instance 信息的可用性。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransformDecompositionError {
    /// 非有限值、非仿射矩阵或计算超出浮点表示范围。
    InvalidMatrix,
    /// 零缩放或奇异矩阵无法可靠恢复旋转。
    Degenerate,
    /// 包含 shear 等无法在容差内由单组 TRS 表示的变换。
    NotTrs,
}

impl TransformTrs {
    const TOLERANCE: f32 = 1e-5;

    /// 复用 glam 分解，并验证重组结果；不修复 shear 或猜测原始负缩放轴。
    ///
    /// glam 的分解不返回错误，必须在调用前排除退化输入，避免触发断言或产生 NaN。
    pub fn try_from_matrix(matrix: Mat4) -> Result<Self, TransformDecompositionError> {
        if !matrix.is_finite() || !matrix.row(3).abs_diff_eq(glam::Vec4::W, Self::TOLERANCE) {
            return Err(TransformDecompositionError::InvalidMatrix);
        }

        let determinant = matrix.determinant();
        let lengths = [matrix.x_axis.length(), matrix.y_axis.length(), matrix.z_axis.length()];
        if !determinant.is_finite() || lengths.iter().any(|length| !length.is_finite()) {
            return Err(TransformDecompositionError::InvalidMatrix);
        }
        if determinant == 0.0 || lengths.iter().any(|length| *length == 0.0 || !length.recip().is_finite()) {
            return Err(TransformDecompositionError::Degenerate);
        }

        let (scale, rotation, translation) = matrix.to_scale_rotation_translation();
        if !scale.is_finite() || !rotation.is_finite() || !translation.is_finite() || !rotation.is_normalized() {
            return Err(TransformDecompositionError::NotTrs);
        }
        let trs = Self {
            translation,
            rotation,
            scale,
        };
        let reconstructed = trs.to_matrix();
        // 逐元素使用相对/绝对混合容差，避免较大的 translation 掩盖线性部分的 shear。
        if !reconstructed.is_finite()
            || !matrix
                .to_cols_array()
                .into_iter()
                .zip(reconstructed.to_cols_array())
                .all(|(original, rebuilt)| {
                    (original - rebuilt).abs() <= Self::TOLERANCE * original.abs().max(rebuilt.abs()).max(1.0)
                })
        {
            return Err(TransformDecompositionError::NotTrs);
        }
        Ok(trs)
    }

    /// 将有限的 TRS 值重组为矩阵；rotation 必须为单位 Quaternion。
    pub fn to_matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.translation)
    }

    /// 返回按 X/Y/Z 排列的 intrinsic XYZ Euler 角度，供只读展示。
    ///
    /// rotation 必须为单位 Quaternion。组合为 Rx * Ry * Rz，对列向量先作用 Rz；
    /// Euler 角不唯一，不承诺恢复导入前的角度，也不经由此显示值重组原矩阵。
    pub fn rotation_euler_degrees(&self) -> Vec3 {
        let (x, y, z) = self.rotation.to_euler(EulerRot::XYZ);
        Vec3::new(x.to_degrees(), y.to_degrees(), z.to_degrees())
    }
}
