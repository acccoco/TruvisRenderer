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

/// 只读位置投影；灯光参数与编辑权威仍属于 GameWorld。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct LightDetailsDto {
    pub scene_version: SceneVersion,
    pub light_id: LightId,
    pub kind: LightKindDto,
    pub position: [f32; 3],
}
