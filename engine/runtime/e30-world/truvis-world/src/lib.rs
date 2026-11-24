//! CPU 场景状态。GameWorld 通过资产 handle 描述实例绑定，交付变更供场景准备使用。

pub mod components;
mod edit_error;
mod game_world;
pub mod guid_new_type;
mod light_parameters;
mod light_target;
pub mod procedural_mesh;

pub use crate::edit_error::{SceneEditError, SceneHandleKind};
pub use crate::game_world::{GameWorld, SceneSkyState};
pub use crate::light_parameters::{AreaLightShape, LightPatch};
pub use crate::light_target::LightTarget;
