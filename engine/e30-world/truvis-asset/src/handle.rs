use std::path::PathBuf;
use std::sync::Arc;

use slotmap::new_key_type;

use ash::vk;

new_key_type! {
    /// 纹理加载任务身份。
    ///
    /// 该 handle 只用于把后台 texture load result 关联回 `SceneAssetIngestor`，
    /// 不表示长期 texture identity、Vulkan image、image view、bindless descriptor
    /// 或 shader 可见 binding。
    pub struct TextureLoadHandle;
}

new_key_type! {
    /// model / prefab 加载任务身份。
    ///
    /// 该 handle 只用于把后台 model import result 关联回 `SceneAssetIngestor`，
    /// 不是长期 model database key，也不是 `SceneStore` 中的 live runtime instance handle。
    pub struct ModelLoadHandle;
}

/// 一次 texture CPU decode task 的输入描述。
///
/// 这是一次性 loader 请求的参数，不是长期 identity key。同一路径是否复用为同一个
/// `TextureHandle` 由 `SceneAssetIngestor` / `SceneStore` 决定。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextureLoadDesc {
    pub path: std::path::PathBuf,
}

/// 一次 model / prefab CPU import task 的输入描述。
///
/// 这是一次性 loader 请求的参数，不表示长期 model database key，也不参与 scene 去重。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelLoadDesc {
    pub path: std::path::PathBuf,
}

/// 解码后的纹理 CPU 像素。
///
/// 两种 payload 都固定为四通道。`Arc` 使 render-side image upload 与异步天空
/// distribution builder 可以共享同一份 32--128 MiB HDR 数据，避免为了跨线程再复制。
#[derive(Debug, Clone)]
pub enum TexturePixels {
    /// 普通图片的 RGBA8 UNORM 像素。
    Rgba8(Arc<[u8]>),
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
/// 当前纹理 bytes 只通过 `AssetLoadEvent::TextureLoaded` 短期交给 `SceneAssetIngestor`
/// 和 render-side texture manager，`AssetHub` 本身不保存像素数据。
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
            TexturePixels::Rgba8(pixels) => pixels.len(),
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
            TexturePixels::Rgba8(_) => vk::Format::R8G8B8A8_UNORM,
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
            TexturePixels::Rgba8(pixels) => pixels,
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
    /// 普通 RGBA8 继续遵循现有 UNORM 上传语义；HDR/EXR 则从 binary16 恢复 scene-linear
    /// 数值。该接口只服务亮度分布构建，不暴露底层 payload 布局。
    pub fn linear_rgb(&self, index: usize) -> [f32; 3] {
        assert!(index < self.texel_count(), "texture texel index out of bounds");
        let channel = index * 4;
        match &self.pixels {
            TexturePixels::Rgba8(pixels) => [
                pixels[channel] as f32 / 255.0,
                pixels[channel + 1] as f32 / 255.0,
                pixels[channel + 2] as f32 / 255.0,
            ],
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
/// `RenderMeshManager` 维护。一个 submesh 是 scene / GPU scene / ray tracing 中
/// 最小的完整几何单元，对应 BLAS 内的一条 geometry。
///
/// 调用方应保持顶点属性数组长度一致，`indices` 使用 `u32` 索引。asset 层不在
/// 注册时重建或修复几何拓扑。
#[derive(Debug, Clone, PartialEq)]
pub struct SubmeshData {
    pub positions: Vec<glam::Vec3>,
    pub normals: Vec<glam::Vec3>,
    pub tangents: Vec<glam::Vec3>,
    pub uvs: Vec<glam::Vec2>,
    pub indices: Vec<u32>,
    pub name: String,
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
    /// 透射表面。`opacity` 只表示透明度，delta / rough 仍只由 roughness 决定。
    Transmission { opacity: f32, ior: f32 },
    /// 自发光表面。radiance 是材质自发光辐亮度，shader 侧会再乘 base color。
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
    /// alpha mask 覆盖。`base_color.w * diffuse_texture_alpha <= alpha_cutoff` 的片元会被忽略。
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
/// texture 仍以导入器返回的路径表达，避免后台 task 直接修改 `SceneStore`。
/// `SceneAssetIngestor` 在 asset sync 阶段解析相对路径、分配 `TextureHandle`
/// 并提交必要的 texture load task。
#[derive(Debug, Clone, PartialEq)]
pub struct RawMaterialData {
    pub base_color: glam::Vec4,
    pub metallic: f32,
    pub roughness: f32,
    pub class: MaterialClass,
    pub coverage: CoverageMode,
    pub diffuse_texture_path: Option<PathBuf>,
    pub normal_texture_path: Option<PathBuf>,
    pub name: String,
}

/// 后台 Assimp task 产出的 owned instance CPU 数据。
///
/// 仍使用导入源内的 mesh/material index，稍后由 `SceneAssetIngestor` 转换成稳定
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
