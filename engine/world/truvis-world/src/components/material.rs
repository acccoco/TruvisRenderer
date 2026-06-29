use crate::guid_new_type::SceneTextureHandle;

/// CPU scene 中的材质语义参数。
///
/// `SceneMaterialData` 是 `World` facade 和 `SceneStore` 对外使用的材质数据形状。
/// texture 引用使用 `SceneTextureHandle`，因此 App、instance、raycast 和 render-side
/// manager 不需要知道 `AssetHub` 内部 loader handle。GPU material slot、bindless
/// texture binding 和 per-FIF material buffer 仍由 `RenderWorld` 内部 manager 维护。
#[derive(Debug, Clone, PartialEq)]
pub struct SceneMaterialData {
    pub base_color: glam::Vec4,
    pub emissive: glam::Vec4,
    pub metallic: f32,
    pub roughness: f32,
    pub opaque: f32,

    pub diffuse_texture: Option<SceneTextureHandle>,
    pub normal_texture: Option<SceneTextureHandle>,
    pub name: String,
}
