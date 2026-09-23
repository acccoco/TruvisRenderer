use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::protocol::{InstanceId, LightId, LightKindDto, MaterialId};

/// Web 可理解的当前网格或灯光 selection。
///
/// instance/material ID 都直接来源于当前 GameWorld SlotMap key；submesh index 是 instance-local
/// 顺序，不表示 GPU geometry slot。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(tag = "type", rename_all = "snake_case")]
pub enum SelectionDto {
    Submesh {
        instance_id: InstanceId,
        submesh_index: u32,
        material_id: MaterialId,
    },
    Light {
        light_id: LightId,
        kind: LightKindDto,
    },
}
