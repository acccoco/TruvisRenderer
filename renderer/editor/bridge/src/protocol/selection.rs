use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::protocol::{InstanceId, MaterialId};

/// Web 可理解的当前 submesh selection。
///
/// instance/material ID 都直接来源于当前 World SlotMap key；submesh index 是 instance-local
/// 顺序，不表示 GPU geometry slot。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SelectionDto {
    pub instance_id: InstanceId,
    pub submesh_index: u32,
    pub material_id: MaterialId,
}
