use truvis_asset::handle::{AssetMaterialHandle, AssetMeshHandle};

/// CPU 侧的 Instance 数据
#[derive(Clone)]
pub struct Instance {
    pub mesh: AssetMeshHandle,
    pub materials: Vec<AssetMaterialHandle>,
    pub transform: glam::Mat4,
}
