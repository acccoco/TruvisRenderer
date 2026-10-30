use crate::components::transform::{TransformDecompositionError, TransformTrs};
use crate::guid_new_type::{MaterialAssetHandle, MeshAssetHandle};

/// CPU 侧的 live instance 语义数据。
///
/// `Instance` 描述一个 runtime object 对哪些 scene mesh / material 形成引用，以及它在
/// world 空间中的变换。这里不保存 GPU buffer、BLAS、descriptor 或稳定 instance slot；
/// 这些资源由 render-side manager / bridge 在 prepare 阶段通过 `SceneStore` 解析。
#[derive(Clone)]
pub struct Instance {
    /// 面向编辑器和调试界面的 CPU scene 展示名称。
    ///
    /// 名称不要求唯一，也不能代替 `MeshInstanceHandle` 作为身份；它随 instance 一起由
    /// `SceneStore` 持有，不会进入 GPU instance buffer。
    pub name: String,

    /// instance 使用的 CPU scene mesh。
    pub mesh: MeshAssetHandle,

    /// instance 使用的 CPU scene material 列表，顺序与 mesh submesh/material slot 对应。
    pub materials: Vec<MaterialAssetHandle>,

    /// CPU 侧 world transform；渲染运行时同步时会把它拷贝到 GPU scene 数据。
    pub transform: glam::Mat4,
}

impl Instance {
    /// World 接受可逆的 affine 矩阵，包括 shear 和负缩放，不要求能分解成编辑器 TRS。
    /// 在 CPU 发布前同时检查 inverse，避免有限输入在后续 GPU 投影时产生非有限值。
    pub(crate) fn validate_transform(transform: glam::Mat4) -> Result<(), String> {
        if !transform.is_finite() || transform.row(3) != glam::Vec4::W {
            return Err("world transform must be finite and affine".into());
        }
        let determinant = transform.determinant();
        if !determinant.is_finite() || determinant == 0.0 || !transform.inverse().is_finite() {
            return Err("world transform must have a finite inverse".into());
        }
        Ok(())
    }

    /// 按需分解 CPU world transform，不保存派生缓存，也不推进 scene version。
    pub fn transform_trs(&self) -> Result<TransformTrs, TransformDecompositionError> {
        TransformTrs::try_from_matrix(self.transform)
    }
}
