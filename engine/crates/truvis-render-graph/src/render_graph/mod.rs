//! RenderGraph：声明式渲染管线编排
//!
//! # 核心约束
//!
//! RenderGraph 只关心 GPU 资源（Image / Buffer），**不感知** Texture、Material 等资产概念。
//! 资产到 GPU 资源的映射由上游模块（AssetHub、BindlessManager 等）在 Upload Phase 完成，
//! RenderGraph 仅消费最终的 `GfxImageHandle` / `GfxBufferHandle`。
//!
//! # 关键设计
//!
//! - **两层 Handle**：`RgImageHandle`（graph 内部虚拟引用）→ `GfxImageHandle`（物理 GPU 资源），
//!   在 compile 阶段建立映射，execute 阶段解引用。
//! - **Pass 添加顺序即逻辑顺序**：用户决定渲染管线的执行顺序，graph 据此建立依赖边。
//! - **自动 Barrier**：compile 阶段通过模拟资源访问序列，自动计算 layout transition 和 memory barrier。

mod barrier;
mod buffer_resource;
mod executor;
mod export_info;
mod graph;
mod image_resource;
mod pass;
mod resource_handle;
mod resource_manager;
mod resource_state;
mod semaphore_info;

// Re-exports
pub use barrier::{BufferBarrierDesc, PassBarriers, RgImageBarrierDesc};
pub use buffer_resource::{RgBufferDesc, RgBufferResource, RgBufferSource};
pub use executor::{CompiledGraph, RenderGraphBuilder};
pub use graph::{DependencyGraph, EdgeData};
pub use image_resource::{RgImageDesc, RgImageResource, RgImageSource};
pub use pass::{RgPass, RgPassBuilder, RgPassContext, RgPassNode};
pub use resource_handle::{RgBufferHandle, RgImageHandle};
pub use resource_manager::RgResourceManager;
pub use resource_state::{RgBufferState, RgImageState};
pub use semaphore_info::RgSemaphoreInfo;
