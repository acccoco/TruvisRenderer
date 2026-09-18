//! 材质纹理槽的纯 CPU 语义；图片身份由导入端或 world 提供，不拥有 GPU 对象。

use crate::handle::TextureColorSpace;

/// 四类核心贴图使用固定顺序，供导入、依赖枚举和材质编辑共用。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub enum TextureChannel {
    BaseColor,
    MetallicRoughness,
    Normal,
    Emissive,
}

impl TextureChannel {
    pub const COUNT: usize = 4;
    pub const ALL: [Self; Self::COUNT] = [Self::BaseColor, Self::MetallicRoughness, Self::Normal, Self::Emissive];

    /// FBX diffuse 映射为 BaseColor，因而与 glTF 颜色槽共用这一解释。
    pub fn color_space(self) -> TextureColorSpace {
        match self {
            Self::BaseColor | Self::Emissive => TextureColorSpace::Srgb,
            Self::MetallicRoughness | Self::Normal => TextureColorSpace::Linear,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::BaseColor => "base_color",
            Self::MetallicRoughness => "metallic_roughness",
            Self::Normal => "normal",
            Self::Emissive => "emissive",
        }
    }
}

/// UV 原点上的 T * R * S。图片与 UV 均采用左上原点，导入器负责转换来源约定。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextureTransform {
    pub offset: glam::Vec2,
    pub rotation: f32,
    pub scale: glam::Vec2,
}

impl Default for TextureTransform {
    fn default() -> Self {
        Self { offset: glam::Vec2::ZERO, rotation: 0.0, scale: glam::Vec2::ONE }
    }
}

impl TextureTransform {
    /// 只在参数变化时计算三角函数；两行仿射系数供 GPU dot(float3(uv, 1)) 使用。
    pub fn affine_rows(self) -> [[f32; 4]; 2] {
        let (sin, cos) = self.rotation.sin_cos();
        [
            [cos * self.scale.x, -sin * self.scale.y, self.offset.x, 0.0],
            [sin * self.scale.x, cos * self.scale.y, self.offset.y, 0.0],
        ]
    }

    pub fn is_finite(self) -> bool {
        self.offset.is_finite() && self.rotation.is_finite() && self.scale.is_finite()
            && self.affine_rows().iter().flatten().all(|value| value.is_finite())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum TextureWrap {
    Repeat,
    Clamp,
    MirroredRepeat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum TextureFilter {
    Nearest,
    Linear,
}

/// 槽级 sampler 描述。同图不同 sampler 不改变图片 identity。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextureSampler {
    pub wrap_s: TextureWrap,
    pub wrap_t: TextureWrap,
    pub filter: TextureFilter,
}

impl Default for TextureSampler {
    fn default() -> Self {
        Self {
            wrap_s: TextureWrap::Repeat,
            wrap_t: TextureWrap::Repeat,
            filter: TextureFilter::Linear,
        }
    }
}

impl TextureSampler {
    /// 有限组合的稳定编码；render-side sampler owner 负责将其映射到缓存表。
    pub fn index(self) -> u32 {
        (self.wrap_s as u32 * 3 + self.wrap_t as u32) * 2 + self.filter as u32
    }
}

/// 一次材质纹理引用。泛型只区分 RawTextureSource 与 TextureHandle，不建立资源管理抽象。
#[derive(Debug, Clone, PartialEq)]
pub struct TextureSlot<T> {
    pub texture: T,
    pub tex_coord: u32,
    pub transform: TextureTransform,
    pub sampler: TextureSampler,
}

impl<T> TextureSlot<T> {
    pub fn new(texture: T) -> Self {
        Self { texture, tex_coord: 0, transform: TextureTransform::default(), sampler: TextureSampler::default() }
    }

    /// Ingest 仅替换资源身份，保留同一次引用的全部采样参数。
    pub fn with_texture<U>(self, texture: U) -> TextureSlot<U> {
        TextureSlot { texture, tex_coord: self.tex_coord, transform: self.transform, sampler: self.sampler }
    }
}
