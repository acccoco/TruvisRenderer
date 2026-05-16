use truvis_asset::handle::{AssetMaterialHandle, AssetMeshHandle};
use truvis_render_interface::render_data::MeshRenderData;

/// asset material handle 到稳定 GPU material slot 的解析接口。
///
/// 由 render-side material bridge 实现，CPU scene 只保存 asset handle，
/// 不接触 texture、bindless 或 GPU material buffer 的细节。
pub(crate) trait MaterialSlotResolver {
    fn resolve_material_slot(&self, handle: AssetMaterialHandle) -> Option<u32>;

    fn is_material_ready(&self, handle: AssetMaterialHandle) -> bool {
        self.resolve_material_slot(handle).is_some()
    }
}

/// asset mesh handle 到 GPU-ready mesh 数据的解析接口。
///
/// 由 render-side mesh uploader 实现，CPU scene 只保存 asset handle，
/// 不接触 vertex/index buffer 上传或 BLAS 构建细节。
pub(crate) trait MeshRenderResolver {
    fn is_mesh_ready(&self, handle: AssetMeshHandle) -> bool {
        self.resolve_mesh(handle).is_some()
    }

    fn resolve_mesh(&self, handle: AssetMeshHandle) -> Option<MeshRenderData<'_>>;
}
