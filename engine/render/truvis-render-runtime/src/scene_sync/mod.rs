//! CPU scene/assets 到 render-side GPU scene 的 prepare 桥接层。

pub(crate) mod asset_mesh_manager;
pub(crate) mod asset_texture_manager;
pub(crate) mod environment_binding;
pub(crate) mod instance_bridge;
pub(crate) mod material_bridge;
pub(crate) mod material_manager;
pub(crate) mod scene_bridge;
pub(crate) mod sky_bridge;
pub(crate) mod texture_resolver;
