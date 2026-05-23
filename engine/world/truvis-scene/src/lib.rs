//! CPU 侧场景语义层。
//!
//! 本 crate 保存 runtime instance、light 和程序化 mesh 的 CPU 数据入口。mesh /
//! material 只通过 `truvis-asset` 的 asset handle 建立引用关系；GPU buffer、BLAS、
//! bindless index、material slot 和稳定 GPU instance slot 都由渲染后端在
//! prepare/sync 阶段解析与维护。
//!
//! `SceneManager` 是 runtime scene 的主入口。它负责把 ready model asset / prefab
//! 转换成 live instance handle，但不直接参与 shader 可见资源的绑定。

pub mod components;
pub mod guid_new_type;
pub mod procedural_mesh;
pub mod scene_manager;
