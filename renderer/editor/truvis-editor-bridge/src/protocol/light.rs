use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::protocol::SceneVersion;

/// 包含灯光类别和 generation 的 session-local opaque ID，Web 不解析其内部结构。
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
pub struct LightId(pub String);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(rename_all = "snake_case")]
pub enum LightKindDto {
    Point,
    Spot,
    Area,
}

/// 灯光参数投影；编辑权威仍属于 GameWorld。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct LightDetailsDto {
    pub scene_version: SceneVersion,
    pub light_id: LightId,
    pub position: [f32; 3],
    pub radiance: [f32; 3],
    pub parameters: LightParametersDto,
}

/// 类别专有参数；Area 形状无法可靠分解时仍可编辑位置和 radiance。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(tag = "kind", rename_all = "snake_case")]
pub enum LightParametersDto {
    Point,
    Spot {
        direction: [f32; 3],
        inner_angle_degrees: f32,
        outer_angle_degrees: f32,
    },
    Area {
        shape: Option<AreaLightShapeDto>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct AreaLightShapeDto {
    pub rotation_degrees: [f32; 3],
    pub width: f32,
    pub height: f32,
}

/// 缺省字段保持 World 最新值，不把旧详情整体回写。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
pub struct LightPatch {
    pub position: Option<[f32; 3]>,
    pub radiance: Option<[f32; 3]>,
    pub direction: Option<[f32; 3]>,
    pub inner_angle_degrees: Option<f32>,
    pub outer_angle_degrees: Option<f32>,
    pub rotation_degrees: Option<[f32; 3]>,
    pub width: Option<f32>,
    pub height: Option<f32>,
}
