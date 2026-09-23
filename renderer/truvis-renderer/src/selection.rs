use truvis_render_runtime::selection::WorldSubmeshSelection;
use truvis_world::{GameWorld, LightTarget, guid_new_type::MaterialAssetHandle};

/// Renderer 唯一选择状态；使用 CPU 身份，不缓存 GPU slot 或材质。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SceneSelection {
    Submesh(WorldSubmeshSelection),
    Light(LightTarget),
}

impl SceneSelection {
    pub fn submesh(self) -> Option<WorldSubmeshSelection> {
        match self {
            Self::Submesh(value) => Some(value),
            Self::Light(_) => None,
        }
    }

    pub fn light(self) -> Option<LightTarget> {
        match self {
            Self::Light(value) => Some(value),
            Self::Submesh(_) => None,
        }
    }

    pub(crate) fn is_valid(self, world: &GameWorld) -> bool {
        let scene = world.scene_view();
        match self {
            Self::Submesh(value) => scene
                .get_instance(value.instance)
                .is_some_and(|instance| (value.submesh_index as usize) < instance.materials.len()),
            Self::Light(value) => scene.light_position(value).is_some(),
        }
    }
}

/// 通知只携带组装 Editor DTO 所需的瞬时数据，不成为第二份 selection。
#[derive(Clone, Copy)]
pub enum SelectionChange {
    Submesh {
        selection: WorldSubmeshSelection,
        material: MaterialAssetHandle,
    },
    Light(LightTarget),
}
