//! backend 私有的 render-side scene 表示。
//!
//! CPU scene 与 asset handle 不直接暴露给 render pass。`InstanceBridge` 先把
//! `SceneManager` 中依赖已就绪的实例整理成 `RenderData` 快照，再由 `GpuScene` 上传为
//! shader 可读 buffer、TLAS 与光栅化 draw cache，最后只通过 `RenderSceneView` 对外读取。

pub(crate) mod geometry;
pub(crate) mod gpu_scene;
pub(crate) mod render_data;
