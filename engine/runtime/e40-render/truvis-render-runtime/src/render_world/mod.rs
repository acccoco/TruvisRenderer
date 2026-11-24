//! runtime 私有的 render-side scene 表示。
//!
//! Render pass 只读取 prepare 后的场景。`RenderInstanceTable` 先把
//! `GameWorld` 中依赖已就绪的实例整理成 `RenderData` 快照，再由 `RenderWorld` 上传为
//! shader 可读 buffer 和光栅化 draw cache，并由内部 `SceneTlas` 更新 TLAS，
//! 最后只通过 `RenderSceneView` 对外读取。

pub(crate) mod buffers;
pub(crate) mod environment_binding;
pub(crate) mod raster_draw_cache;
pub(crate) mod analytic_light_table;
pub(crate) mod render_data;
pub(crate) mod render_emissive_light_table;
pub(crate) mod render_instance_table;
pub(crate) mod gpu_sky_store;
pub(crate) mod scene_tlas;
pub(crate) mod render_world;
pub(crate) mod sky_distribution_builder;
