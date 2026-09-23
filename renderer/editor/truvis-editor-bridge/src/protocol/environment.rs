use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::protocol::{SceneVersion, TextureId};

/// 场景单例环境；CPU 加载阶段不等价于 GPU 可见。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct EnvironmentDetailsDto {
    pub scene_version: SceneVersion,
    pub enabled: bool,
    pub brightness: f32,
    pub texture_id: Option<TextureId>,
    pub file_name: Option<String>,
    pub load_state: EnvironmentLoadState,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(rename_all = "snake_case")]
pub enum EnvironmentLoadState {
    Unset,
    Loading,
    Ready,
    Failed,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
pub struct EnvironmentPatch {
    pub enabled: Option<bool>,
    pub brightness: Option<f32>,
}
