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

/// Web 可读取的完整材质 DTO。
///
/// texture 字段仍是 World texture handle 的 opaque ID；当前第一阶段 Web 只展示绑定，
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
    pub diffuse_texture: Option<TextureId>,
    pub normal_texture: Option<TextureId>,
}

/// `UpdateMaterial` 的绝对赋值 patch。
///
/// 缺失字段保持当前 World 值；存在字段必须通过 App 侧数值与领域校验。Web 在用户松开
/// 鼠标时发送一次 patch，不在拖动期间持续发送 preview command。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
pub struct MaterialPatch {
    pub name: Option<String>,
    pub base_color: Option<[f32; 4]>,
    pub metallic: Option<f32>,
    pub roughness: Option<f32>,
    pub class: Option<MaterialClassDto>,
    pub coverage: Option<CoverageModeDto>,
}
