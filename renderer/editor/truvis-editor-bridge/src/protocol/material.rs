use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::protocol::{MaterialId, TextureId};

/// Web 协议中的材质光学类别。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(tag = "kind", rename_all = "snake_case")]
pub enum MaterialClassDto {
    Surface,
    Transmission { opacity: f32, ior: f32 },
    Emissive { radiance: [f32; 3] },
}

/// Web 协议中的材质覆盖模式。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(tag = "kind", rename_all = "snake_case")]
pub enum CoverageModeDto {
    Opaque,
    AlphaMask { alpha_cutoff: f32 },
}

/// 槽级采样参数；rotation 使用弧度，UI 自行转换角度显示。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct TextureMappingDto {
    pub tex_coord: u32,
    pub offset: [f32; 2],
    pub rotation: f32,
    pub scale: [f32; 2],
    /// 0 Repeat、1 Clamp、2 MirroredRepeat。
    pub wrap: [u32; 2],
    /// 0 Nearest、1 Linear。
    pub filter: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct TextureSlotDto {
    pub texture: TextureId,
    pub mapping: TextureMappingDto,
}

/// 只编辑指定槽的映射，不替换纹理身份，也不覆盖其它槽。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct TextureMappingPatch {
    pub channel: u32,
    pub mapping: TextureMappingDto,
}

/// Web 可读取的完整材质 DTO。
///
/// texture 字段仍是 GameWorld texture handle 的 opaque ID；当前第一阶段 Web 只展示绑定，
/// 不直接上传纹理或访问 GPU bindless handle。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct MaterialDto {
    pub id: MaterialId,
    pub name: String,
    pub base_color: [f32; 4],
    pub metallic: f32,
    pub roughness: f32,
    pub class: MaterialClassDto,
    pub coverage: CoverageModeDto,
    pub textures: [Option<TextureSlotDto>; 4],
    pub normal_scale: f32,
    pub emissive_factor: [f32; 3],
}

/// `UpdateMaterial` 的绝对赋值 patch。
///
/// 缺失字段保持当前 GameWorld 值；存在字段必须通过 Renderer 侧数值与领域校验。Web 在用户松开
/// 鼠标时发送一次 patch，不在拖动期间持续发送 preview command。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
pub struct MaterialPatch {
    pub name: Option<String>,
    pub base_color: Option<[f32; 4]>,
    pub metallic: Option<f32>,
    pub roughness: Option<f32>,
    pub class: Option<MaterialClassDto>,
    pub coverage: Option<CoverageModeDto>,
    pub texture_mappings: Option<Vec<TextureMappingPatch>>,
    pub normal_scale: Option<f32>,
    pub emissive_factor: Option<[f32; 3]>,
}
