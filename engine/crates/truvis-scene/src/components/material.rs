use truvis_asset::handle::AssetTextureHandle;

/// CPU 侧的材质数据
#[derive(Default)]
pub struct Material {
    pub base_color: glam::Vec4,
    pub emissive: glam::Vec4,
    pub metallic: f32,
    pub roughness: f32,
    pub opaque: f32,

    pub diffuse_map: String,
    pub normal_map: String,
}

/// MaterialManager 使用的 CPU 侧材质参数
///
/// 与 `Material` 的区别：texture 字段使用 `AssetTextureHandle` 而非路径字符串，
/// 支持异步加载和 bindless 绑定。
#[derive(Clone)]
pub struct ManagedMaterialParams {
    pub base_color: glam::Vec4,
    pub emissive: glam::Vec4,
    pub metallic: f32,
    pub roughness: f32,
    pub opaque: f32,

    pub diffuse_texture: Option<AssetTextureHandle>,
    pub normal_texture: Option<AssetTextureHandle>,
}

impl Default for ManagedMaterialParams {
    fn default() -> Self {
        Self {
            base_color: glam::Vec4::ONE,
            emissive: glam::Vec4::ZERO,
            metallic: 0.0,
            roughness: 0.5,
            opaque: 1.0,
            diffuse_texture: None,
            normal_texture: None,
        }
    }
}
