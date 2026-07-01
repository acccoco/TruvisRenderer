use truvis_world::guid_new_type::{MaterialHandle, MeshHandle};

use crate::render_world::render_data::MeshRenderData;

/// scene material handle 到稳定 GPU material slot 的解析接口。
///
/// 由 render-side material manager 实现，CPU scene 只保存 scene material handle，
/// 不接触 texture、bindless 或 GPU material buffer 的细节。
pub(crate) trait MaterialSlotResolver {
    /// 解析 shader 可索引的稳定 material slot。
    ///
    /// 返回 None 表示材质尚未进入 render-side manager，依赖它的 instance 不应激活。
    fn resolve_material_slot(&self, handle: MaterialHandle) -> Option<u32>;

    /// 判断 material 是否已经拥有稳定 slot。
    ///
    /// texture 是否真实 ready 不影响该判断；未 ready texture 由 material manager fallback 兜底。
    fn is_material_ready(&self, handle: MaterialHandle) -> bool {
        self.resolve_material_slot(handle).is_some()
    }
}

/// scene mesh handle 到 GPU-ready mesh 数据的解析接口。
///
/// 由 render-side mesh manager 实现，CPU scene 只保存 scene mesh handle，
/// 不接触 vertex/index buffer 上传或 BLAS 构建细节。
pub(crate) trait MeshRenderResolver {
    /// 判断 mesh 是否已经完成 vertex/index 上传和 BLAS build。
    fn is_mesh_ready(&self, handle: MeshHandle) -> bool {
        self.resolve_mesh(handle).is_some()
    }

    /// 解析 GPU-ready mesh 数据引用。
    ///
    /// 返回的数据由 mesh manager 持有生命周期，`RenderData` 只在 prepare 阶段借用它。
    fn resolve_mesh(&self, handle: MeshHandle) -> Option<MeshRenderData<'_>>;
}
