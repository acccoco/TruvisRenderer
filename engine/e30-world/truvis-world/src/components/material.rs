use crate::guid_new_type::TextureHandle;

pub use truvis_asset::handle::{CoverageMode, MaterialClass};
pub use truvis_asset::material_texture::{TextureChannel, TextureSlot};

/// CPU scene 中的材质语义参数。
///
/// `MaterialData` 是 `GameWorld` facade 和 `SceneStore` 对外使用的材质数据形状。
/// texture 引用使用 `TextureHandle`，因此 Renderer、instance、raycast 和 render-side
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

    pub textures: [Option<TextureSlot<TextureHandle>>; TextureChannel::COUNT],
    pub normal_scale: f32,
    pub emissive_factor: glam::Vec3,
    pub name: String,
}

impl Default for MaterialData {
    fn default() -> Self {
        Self {
            base_color: glam::Vec4::ONE,
            metallic: 0.0,
            roughness: 1.0,
            class: MaterialClass::Surface,
            coverage: CoverageMode::Opaque,
            textures: Default::default(),
            normal_scale: 1.0,
            emissive_factor: glam::Vec3::ZERO,
            name: String::new(),
        }
    }
}

impl MaterialData {
    /// 在任何 CPU edit 提交前校验，防止 NaN 破坏相等比较与跨帧版本对账。
    pub fn validate(&self) -> Result<(), String> {
        if !self.base_color.is_finite() || !self.metallic.is_finite() || !self.roughness.is_finite()
            || !self.normal_scale.is_finite()
            || !self.emissive_factor.is_finite() || self.emissive_factor.min_element() < 0.0
        {
            return Err(format!("material '{}' has invalid factors", self.name));
        }
        for (channel, slot) in TextureChannel::ALL.into_iter().zip(&self.textures) {
            if slot.as_ref().is_some_and(|slot| !slot.transform.is_finite()) {
                return Err(format!("material '{}' {} has non-finite transform", self.name, channel.name()));
            }
        }
        Ok(())
    }

    /// 对当前绑定的每个 submesh 分别检查，失败不能静默选择 UV0。
    pub fn validate_uv_sets(&self, submesh: &truvis_asset::handle::SubmeshData) -> Result<(), String> {
        for (channel, slot) in TextureChannel::ALL.into_iter().zip(&self.textures) {
            if let Some(slot) = slot {
                if slot.tex_coord as usize >= submesh.tex_coords.len() {
                    return Err(format!("material '{}' {} requires TEXCOORD_{} missing from submesh '{}'",
                        self.name, channel.name(), slot.tex_coord, submesh.name));
                }
            }
        }
        Ok(())
    }
}
