//! runtime 私有的 render-side scene 表示。
//!
//! CPU scene 与 loader handle 不直接暴露给 render pass。`RenderInstanceManager` 先把
//! `SceneStore` 中依赖已就绪的实例整理成 `RenderData` 快照，再由 `RenderWorld` 上传为
//! shader 可读 buffer 和光栅化 draw cache，并由内部 `RenderTlasManager` 更新 TLAS，
//! 最后只通过 `RenderSceneView` 对外读取。

pub(crate) mod buffers;
pub(crate) mod environment_binding;
pub(crate) mod geometry;
pub(crate) mod raster_draw_cache;
pub(crate) mod render_data;
pub(crate) mod render_emissive_light_table;
pub(crate) mod render_instance_manager;
pub(crate) mod render_material_manager;
pub(crate) mod render_mesh_manager;
pub(crate) mod render_resolver;
pub(crate) mod render_sky_manager;
pub(crate) mod render_texture_manager;
pub(crate) mod render_tlas_manager;
pub(crate) mod render_world;
pub(crate) mod texture_resolver;
