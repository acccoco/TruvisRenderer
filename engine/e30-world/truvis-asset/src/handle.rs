use std::path::PathBuf;
use std::sync::Arc;

use slotmap::new_key_type;

use ash::vk;

new_key_type! {
    /// 纹理加载任务身份。
    ///
    /// 该 handle 只用于把后台 texture load result 关联回 `AssetSystem`，
    /// 不表示长期 texture identity、Vulkan image、image view、bindless descriptor
    /// 或 shader 可见 binding。
    pub struct TextureLoadHandle;
}

new_key_type! {
    /// scene / prefab 加载任务身份。
    ///
    /// 该 handle 只用于把后台 scene import result 关联回 `AssetSystem`，
    /// 不是长期 scene database key，也不是 `SceneStore` 中的 live runtime instance handle。
    pub struct SceneLoadHandle;
}

/// RGBA8 图片的输入解释。颜色空间来自纹理用途，不能从文件名或图片 metadata 猜测。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextureColorSpace {
    Linear,
    Srgb,
}

/// 一次 texture CPU decode task 的输入描述。
///
/// 这是一次性 loader 请求的参数，不是长期 identity key。同一路径是否复用为同一个
/// `TextureAssetHandle` 由 `AssetSystem` 内部的 `AssetStore` 决定。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextureLoadDesc {
    File { path: PathBuf, color_space: TextureColorSpace },
    Embedded {
        identity: EmbeddedTextureId,
        bytes: Arc<[u8]>,
        mime_type: Option<String>,
        color_space: TextureColorSpace,
    },
}

impl TextureLoadDesc {
    pub fn color_space(&self) -> TextureColorSpace {
        match self {
            Self::File { color_space, .. } | Self::Embedded { color_space, .. } => *color_space,
        }
    }

    /// 返回不包含像素 payload 的诊断文本，避免把 embedded bytes 写入日志。
    pub fn source_label(&self) -> String {
        match self {
            Self::File { path, color_space } => format!("file:{},color_space:{color_space:?}", path.display()),
            Self::Embedded {
                identity,
                mime_type,
                bytes,
                color_space,
            } => format!(
                "embedded:image={},mime={},bytes={},color_space:{color_space:?}",
                identity.image_index,
                mime_type.as_deref().unwrap_or("unknown"),
                bytes.len()
            ),
        }
    }
}

/// embedded image 在一个 scene document 内的稳定身份。
///
/// 跨 scene 的去重由 `AssetSystem` 额外组合 canonical scene path；这里不对
/// encoded bytes 做内容哈希，也不承担全局 asset database 的职责。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EmbeddedTextureId {
    pub image_index: u32,
}

/// 一次 scene / prefab CPU import task 的输入描述。
///
/// 这是一次性 loader 请求的参数，不表示长期 scene database key，也不参与 scene 去重。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneLoadDesc {
    pub path: std::path::PathBuf,
}

/// 解码后的纹理 CPU 像素。
///
/// 两种 payload 都固定为四通道。`Arc` 使 render-side image upload 与异步天空
/// distribution builder 可以共享同一份 32--128 MiB HDR 数据，避免为了跨线程再复制。
#[derive(Debug, Clone)]
pub enum TexturePixels {
    /// 普通图片的原始 RGBA8 数值；采样格式由引用它的用途决定。
    Rgba8 { pixels: Arc<[u8]>, color_space: TextureColorSpace },
    /// HDR/EXR 的 RGBA16F 像素；每个 `u16` 保存 IEEE-754 binary16 bit pattern。
    Rgba16Float(Arc<[u16]>),
}

/// 解码后的纹理 CPU 数据。
///
/// 这是 asset 层传给渲染运行时 texture manager 的边界格式：像素已经位于 owned、
/// 可共享的 CPU buffer，但还没有创建 image、image view 或 bindless descriptor。
/// Vulkan format 只能由 `TexturePixels` 推导，调用方不能单独指定，因而不会出现
/// pixel payload 与 GPU format 不一致的状态。
///
/// 当前纹理 bytes 只通过 `AssetLoadEvent::TextureLoaded` 短期交给 `AssetSystem`
/// 和 render-side texture manager，`AssetLoadService` 本身不保存像素数据。
#[derive(Debug, Clone)]
pub struct TextureBytes {
    pixels: TexturePixels,
    extent: vk::Extent3D,
}

impl TextureBytes {
    /// 创建并校验 upload-ready texture payload。
    ///
    /// extent 必须是非空 2D image，`pixels` 严格包含 `width * height * 4` 个通道元素。校验集中在
    /// asset 边界，后续 staging upload 和 sky distribution 可以依赖该不变量。
    pub fn new(pixels: TexturePixels, extent: vk::Extent3D) -> Result<Self, String> {
        if extent.width == 0 || extent.height == 0 || extent.depth != 1 {
            return Err(format!("texture extent must be a non-empty 2D image: {extent:?}"));
        }
        let texel_count = (extent.width as usize)
            .checked_mul(extent.height as usize)
            .ok_or_else(|| format!("texture extent overflows usize: {extent:?}"))?;
        let expected_channel_count =
            texel_count.checked_mul(4).ok_or_else(|| format!("texture channel count overflows usize: {extent:?}"))?;
        let actual_channel_count = match &pixels {
            TexturePixels::Rgba8 { pixels, .. } => pixels.len(),
            TexturePixels::Rgba16Float(pixels) => pixels.len(),
        };
        if actual_channel_count != expected_channel_count {
            return Err(format!(
                "texture pixel count mismatch: extent={extent:?}, expected_channels={expected_channel_count}, actual_channels={actual_channel_count}"
            ));
        }

        Ok(Self { pixels, extent })
    }

    #[inline]
    pub fn format(&self) -> vk::Format {
        match &self.pixels {
            TexturePixels::Rgba8 { color_space: TextureColorSpace::Linear, .. } => vk::Format::R8G8B8A8_UNORM,
            TexturePixels::Rgba8 { color_space: TextureColorSpace::Srgb, .. } => vk::Format::R8G8B8A8_SRGB,
            TexturePixels::Rgba16Float(_) => vk::Format::R16G16B16A16_SFLOAT,
        }
    }

    /// 返回 staging upload 使用的原始字节视图。
    ///
    /// `Rgba16Float` 已保存为 native-endian `u16` bit pattern；Vulkan staging copy
    /// 只关心原始 bytes，因此这里使用 bytemuck 的安全 slice cast，不产生新分配。
    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        match &self.pixels {
            TexturePixels::Rgba8 { pixels, .. } => pixels,
            TexturePixels::Rgba16Float(pixels) => bytemuck::cast_slice(pixels),
        }
    }

    #[inline]
    pub fn extent(&self) -> vk::Extent3D {
        self.extent
    }

    #[inline]
    pub fn texel_count(&self) -> usize {
        self.extent.width as usize * self.extent.height as usize
    }

    /// 按 texel 索引读取线性 RGB。
    ///
    /// sRGB RGBA8 先按 RGB transfer function 解码，Alpha 不参与；HDR/EXR 则从
    /// binary16 恢复 scene-linear 数值。该接口只服务亮度分布构建。
    pub fn linear_rgb(&self, index: usize) -> [f32; 3] {
        assert!(index < self.texel_count(), "texture texel index out of bounds");
        let channel = index * 4;
        match &self.pixels {
            TexturePixels::Rgba8 { pixels, color_space } => [pixels[channel], pixels[channel + 1], pixels[channel + 2]]
                .map(|byte| {
                    let value = byte as f32 / 255.0;
                    match color_space {
                        TextureColorSpace::Linear => value,
                        TextureColorSpace::Srgb if value <= 0.04045 => value / 12.92,
                        TextureColorSpace::Srgb => ((value + 0.055) / 1.055).powf(2.4),
                    }
                }),
            TexturePixels::Rgba16Float(pixels) => [
                half::f16::from_bits(pixels[channel]).to_f32(),
                half::f16::from_bits(pixels[channel + 1]).to_f32(),
                half::f16::from_bits(pixels[channel + 2]).to_f32(),
            ],
        }
    }
}

/// upload-ready 的 CPU submesh 数据。
///
/// 数据已经从导入库的临时内存复制到 Rust owned buffer。asset 层在这里停止，
/// 后续的 vertex/index buffer 创建、BLAS geometry 构建和 GPU ready 状态由
/// `GpuMeshStore` 维护。一个 submesh 是 scene / GPU scene / ray tracing 中
/// 最小的完整几何单元，对应 BLAS 内的一条 geometry。
///
/// 调用方应保持顶点属性数组长度一致，`indices` 使用 `u32` 索引。asset 层不在
/// 注册时重建或修复几何拓扑。
#[derive(Debug, Clone, PartialEq)]
pub struct SubmeshData {
    pub positions: Vec<glam::Vec3>,
    pub normals: Vec<glam::Vec3>,
    /// XYZW 保留切线方向和 bitangent handedness；零向量表示需要从所选 UV 重建。
    pub tangents: Vec<glam::Vec4>,
    /// 每套 UV 都与 positions 等长，按 glTF TEXCOORD_n 索引；左上原点。
    pub tex_coords: Vec<Vec<glam::Vec2>>,
    /// 导入切线对应的 UV 集；切换法线槽 UV 时不能误用其它参数化的切线。
    pub tangent_tex_coord: u32,
    pub indices: Vec<u32>,
    pub name: String,
}

impl SubmeshData {
    /// 导入和 CPU 注册共用 GPU 上传前置条件；失败时不得上传不完整属性或越界索引。
    pub fn validate(&self) -> Result<(), String> {
        let count = self.positions.len();
        let reason = if count == 0 {
            Some("has no vertices")
        } else if count > u32::MAX as usize
            || self.tex_coords.len().checked_mul(count).is_none_or(|size| size > u32::MAX as usize)
        {
            Some("exceeds GPU uint vertex/UV addressing range")
        } else if self.normals.len() != count || self.tangents.len() != count
            || self.tex_coords.iter().any(|set| set.len() != count)
        {
            Some("has mismatched vertex attribute counts")
        } else if self.positions.iter().any(|p| !p.is_finite())
            || self.normals.iter().any(|n| !n.is_finite() || n.length_squared() <= f32::EPSILON)
            || self.tangents.iter().any(|t| !t.is_finite())
            || self.tex_coords.iter().flatten().any(|uv| !uv.is_finite())
        {
            Some("has non-finite attributes or zero normals")
        } else if self.indices.is_empty() || !self.indices.len().is_multiple_of(3) {
            Some("must contain triangle indices")
        } else if self.indices.iter().any(|&vertex| vertex as usize >= count) {
            Some("has out-of-range vertex indices")
        } else {
            None
        };
        reason.map_or(Ok(()), |reason| Err(format!("submesh '{}' {}", self.name, reason)))
    }
}

/// upload-ready 的 CPU mesh 数据。
///
/// `MeshData` 是 `SceneStore` 与 render-side mesh manager 之间的 mesh 边界格式：
/// mesh 本身对应一个 BLAS，内部每个 `SubmeshData` 对应 BLAS 中的一条 geometry。
/// instance 只能引用一个 mesh，并且它的 material 列表必须与这里的 submesh 顺序一一对齐。
#[derive(Debug, Clone, PartialEq)]
pub struct MeshData {
    pub name: String,
    pub submeshes: Vec<SubmeshData>,
}

impl MeshData {
    /// 把现有单几何导入路径显式包装成单 submesh mesh。
    ///
    /// 该 helper 只用于保持 importer / procedural mesh 当前“一份几何就是一个 mesh”的策略；
    /// 长期 scene 语义仍以 `submeshes` 为准，不能再把 `MeshData` 本身当成几何体。
    pub fn from_single_submesh(submesh: SubmeshData) -> Self {
        Self {
            name: submesh.name.clone(),
            submeshes: vec![submesh],
        }
    }

    #[inline]
    pub fn submesh_count(&self) -> usize {
        self.submeshes.len()
    }
}

/// CPU 材质的光学类别。
///
/// `MaterialClass` 只表达命中表面后如何解释光学事件；alpha mask 可见性由
/// `CoverageMode` 单独表达。它是 CPU scene、GPU material buffer、closest-hit
/// 分类和 emissive light table 的共同语义来源，不直接决定 TLAS any-hit。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MaterialClass {
    /// 普通表面。是否 delta / rough 由 shader 根据 roughness 决定。
    Surface,
    /// 透射表面。光滑界面的 `opacity` 只吸收透射能量，不转为反射；
    /// opacity 与 base color 逐界面作用，不表示按厚度计算的体积吸收。
    /// delta / rough 仍只由 roughness 决定；当前 rough BTDF 尚未实现。
    Transmission { opacity: f32, ior: f32 },
    /// 自发光表面。radiance 与 emissive factor 相加后乘 emissive 纹理，独立于 base color。
    Emissive { radiance: glam::Vec3 },
}

impl MaterialClass {
    /// glTF / 通用玻璃材质未显式指定 IOR 时的标准默认值。
    pub const DEFAULT_IOR: f32 = 1.5;

    pub fn transmission(opacity: f32, ior: f32) -> Self {
        Self::Transmission {
            opacity: opacity.clamp(0.0, 1.0),
            ior: ior.max(1.0),
        }
    }

    pub fn emissive(radiance: glam::Vec3) -> Self {
        Self::Emissive {
            radiance: radiance.max(glam::Vec3::ZERO),
        }
    }

    #[inline]
    pub fn opacity(self) -> f32 {
        match self {
            Self::Transmission { opacity, .. } => opacity,
            Self::Surface | Self::Emissive { .. } => 1.0,
        }
    }

    #[inline]
    pub fn ior(self) -> f32 {
        match self {
            Self::Transmission { ior, .. } => ior,
            Self::Surface | Self::Emissive { .. } => 1.0,
        }
    }

    #[inline]
    pub fn emissive_radiance(self) -> glam::Vec3 {
        match self {
            Self::Emissive { radiance } => radiance,
            Self::Surface | Self::Transmission { .. } => glam::Vec3::ZERO,
        }
    }

    #[inline]
    pub fn is_emissive(self) -> bool {
        matches!(self, Self::Emissive { .. })
    }
}

/// CPU 材质的表面覆盖模式。
///
/// Coverage 只决定三角形候选是否需要 alpha test。它不改变材质光学类别，因此同一个
/// `MaterialClass` 可以在 v1 中与 Opaque 或 AlphaMask 组合；TLAS any-hit 只从这里派生。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CoverageMode {
    /// 普通覆盖，`base_color.w` 不参与可见性判断。
    Opaque,
    /// alpha mask 覆盖。`base_color.w * base_color_texture_alpha < alpha_cutoff` 的片元会被忽略。
    AlphaMask { alpha_cutoff: f32 },
}

impl CoverageMode {
    /// glTF alpha mask 未显式指定 cutoff 时的标准默认值。
    pub const DEFAULT_ALPHA_CUTOFF: f32 = 0.5;

    pub fn alpha_mask(alpha_cutoff: f32) -> Self {
        Self::AlphaMask {
            alpha_cutoff: alpha_cutoff.clamp(0.0, 1.0),
        }
    }

    #[inline]
    pub fn requires_any_hit(self) -> bool {
        matches!(self, Self::AlphaMask { .. })
    }

    #[inline]
    pub fn alpha_cutoff(self) -> f32 {
        match self {
            Self::AlphaMask { alpha_cutoff } => alpha_cutoff,
            Self::Opaque => 0.0,
        }
    }
}

/// 后台 Assimp task 产出的 owned material CPU 数据。
///
/// texture 仍以导入器返回的 source 表达，避免后台 task 直接修改 `SceneStore`。
/// `AssetSystem` 在 asset sync 阶段解析相对路径、分配 `TextureAssetHandle`
/// 并提交必要的 file 或 memory texture load task。
#[derive(Debug, Clone, PartialEq)]
pub struct RawMaterialData {
    pub base_color: glam::Vec4,
    pub metallic: f32,
    pub roughness: f32,
    pub class: MaterialClass,
    pub coverage: CoverageMode,
    pub textures: [Option<crate::material_texture::TextureSlot<RawTextureSource>>; crate::material_texture::TextureChannel::COUNT],
    pub normal_scale: f32,
    /// 独立于光学类别的发光因子；有发光贴图时按其 RGB 相乘。
    pub emissive_factor: glam::Vec3,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RawTextureSource {
    ExternalPath(PathBuf),
    Embedded {
        identity: EmbeddedTextureId,
        bytes: Arc<[u8]>,
        mime_type: Option<String>,
    },
}

/// 后台 Assimp task 产出的 owned instance CPU 数据。
///
/// 仍使用导入源内的 mesh/material index，稍后由 `AssetSystem` 转换成稳定
/// scene handle，避免把半成品 handle 分配逻辑放入 FFI copy 任务。
#[derive(Debug, Clone, PartialEq)]
pub struct RawSceneInstanceData {
    pub mesh_index: u32,
    pub material_indices: Vec<u32>,
    pub transform: glam::Mat4,
    pub name: String,
}

/// 后台 Assimp task 产出的 owned scene CPU 数据。
///
/// 这里不保存任何 C++ handle 或 raw pointer。Assimp / C++ scene 的生命周期
/// 被限制在后台 task 内，`truvixx_scene_free` 已经在返回该结构前完成。
#[derive(Debug, Clone, PartialEq)]
pub struct RawSceneData {
    pub source_path: PathBuf,
    pub name: String,
    pub meshes: Vec<MeshData>,
    pub materials: Vec<RawMaterialData>,
    pub instances: Vec<RawSceneInstanceData>,
}

/// asset 层的 CPU 加载状态机。
///
/// 对 loader 而言，`Ready` 只表示 CPU 侧数据已经通过 event 交付给上层。
/// 纹理是否已经注册 bindless、mesh 是否已有 GPU buffer / BLAS、material 是否已有 GPU slot，
/// 都由渲染运行时自己的 manager 再维护一层 ready 状态。
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum LoadStatus {
    /// 初始状态，资源尚未请求加载。
    Unloaded,
    /// IO / CPU 阶段：后台线程正在读取文件、解码纹理或导入 model。
    Loading,
    /// CPU 完成状态：数据已经通过完成事件交付。
    Ready,
    /// 失败状态：文件不存在、格式错误、解码失败或导入器返回错误。
    Failed,
}
