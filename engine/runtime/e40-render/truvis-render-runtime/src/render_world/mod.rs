//! runtime 私有的 render-side scene 表示。
//!
//! CPU scene 与 loader handle 不直接暴露给 render pass。`RenderInstanceTable` 先把
//! `SceneStore` 中依赖已就绪的实例整理成 `RenderData` 快照，再由 `RenderWorld` 上传为
//! shader 可读 buffer 和光栅化 draw cache，并由内部 `SceneTlas` 更新 TLAS，
//! 最后只通过 `RenderSceneView` 对外读取。

pub(crate) mod buffers;
pub(crate) mod environment_binding;
pub(crate) mod geometry;
pub(crate) mod raster_draw_cache;
pub(crate) mod analytic_light_table;
pub(crate) mod gpu_asset_upload_queue;
pub(crate) mod render_data;
pub(crate) mod render_emissive_light_table;
pub(crate) mod render_instance_table;
pub(crate) mod gpu_material_store;
pub(crate) mod gpu_mesh_store;
pub(crate) mod render_asset_system;
pub(crate) mod render_resolver;
pub(crate) mod gpu_sky_store;
pub(crate) mod gpu_texture_store;
pub(crate) mod scene_tlas;
pub(crate) mod render_world;
pub(crate) mod sky_distribution_builder;
pub(crate) mod texture_resolver;
