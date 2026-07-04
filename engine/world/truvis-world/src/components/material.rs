use crate::guid_new_type::TextureHandle;

pub use truvis_asset::handle::{CoverageMode, MaterialClass};

/// CPU scene 中的材质语义参数。
///
/// `MaterialData` 是 `World` facade 和 `SceneStore` 对外使用的材质数据形状。
/// texture 引用使用 `TextureHandle`，因此 App、instance、raycast 和 render-side
/// manager 不需要知道 `AssetHub` 内部 loader handle。GPU material slot、bindless
/// texture binding 和 per-FIF material buffer 仍由 `RenderWorld` 内部 manager 维护。
/// `class` 是 CPU -> GPU 光学类别的权威来源；`coverage` 单独表达 alpha mask 可见性。
/// `base_color.w` 只作为 `CoverageMode::AlphaMask` 的 alpha factor，与 diffuse 贴图 alpha
/// 相乘后再和 cutoff 比较。
#[derive(Debug, Clone, PartialEq)]
pub struct MaterialData {
    pub base_color: glam::Vec4,
    pub metallic: f32,
    pub roughness: f32,
    pub class: MaterialClass,
    pub coverage: CoverageMode,

    pub diffuse_texture: Option<TextureHandle>,
    pub normal_texture: Option<TextureHandle>,
    pub name: String,
}
