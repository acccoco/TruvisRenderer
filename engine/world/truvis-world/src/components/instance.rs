use crate::guid_new_type::{SceneMaterialHandle, SceneMeshHandle};

/// CPU 侧的 live instance 语义数据。
///
/// `Instance` 描述一个 runtime object 对哪些 scene mesh / material 形成引用，以及它在
/// world 空间中的变换。这里不保存 GPU buffer、BLAS、descriptor 或稳定 instance slot；
/// 这些资源由 render-side manager / bridge 在 prepare 阶段通过 `SceneStore` 解析。
#[derive(Clone)]
pub struct Instance {
    /// instance 使用的 CPU scene mesh。
    pub mesh: SceneMeshHandle,
    /// instance 使用的 CPU scene material 列表，顺序与 mesh submesh/material slot 对应。
    pub materials: Vec<SceneMaterialHandle>,
    /// CPU 侧 world transform；渲染运行时同步时会把它拷贝到 GPU scene 数据。
    pub transform: glam::Mat4,
}
