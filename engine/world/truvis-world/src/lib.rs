//! CPU 侧 world 聚合层。
//!
//! `World` 是 update 阶段和 render runtime prepare 阶段之间的 CPU 数据入口，聚合
//! runtime scene 状态与 `truvis-asset` 的内容资产状态。它不拥有 Vulkan、swapchain、
//! GPU buffer/image、frame state 或 shader binding 资源；这些对象由 render-side runtime 管理。

use truvis_asset::asset_hub::AssetHub;

pub mod components;
pub mod guid_new_type;
pub mod procedural_mesh;
pub mod scene_manager;

use crate::scene_manager::SceneManager;

/// CPU 侧场景状态的聚合容器。
///
/// 与 GPU-facing 状态物理分离，建立 CPU/GPU 数据的所有权边界。App /
/// Plugin 在 update 阶段通过这里修改 CPU state；`RenderRuntime::prepare` 再读取这些数据，
/// 同步到 render-side manager、bridge、`GpuScene` 和 shader-visible bindings。
pub struct World {
    /// runtime scene 语义数据，包括 live instance 和 light。
    ///
    /// 这里的 handle 是 CPU runtime 身份，不是 GPU slot；渲染运行时负责把它们同步到
    /// GPU-visible scene 数据。
    pub scene_manager: SceneManager,
    /// 内容资产入口和 CPU 加载状态管理。
    ///
    /// `AssetHub` 负责 asset handle、去重和 ready 数据汇聚；texture/image、mesh buffer、
    /// BLAS、material slot 等 GPU 资源由 render runtime 根据这里的状态继续创建。
    pub asset_hub: AssetHub,
}
