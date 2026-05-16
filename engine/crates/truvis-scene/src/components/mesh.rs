use truvis_asset::handle::AssetMeshHandle;

/// CPU 侧的 Mesh 数据
pub struct Mesh {
    /// 内容资产 mesh 身份。GPU buffer / BLAS 由 render-side uploader 持有。
    pub asset_mesh: AssetMeshHandle,
    pub name: String,
}
