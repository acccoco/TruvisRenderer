//! RenderGraph：按帧命令录制与同步辅助
//!
//! # 核心约束
//!
//! RenderGraph 只关心已导入的 GPU Image，**不感知** Texture、Material 等资产概念。
//! 资产到 GPU 资源的映射由上游模块（AssetHub、BindlessManager 等）在 Upload Phase 完成，
//! RenderGraph 仅消费最终的 `GfxImageHandle`。
//!
//! # 关键设计
//!
//! - **两层 Handle**：`RgImageHandle`（graph 内部虚拟引用）→ `GfxImageHandle`（物理 GPU 资源），
//!   在 compile 阶段建立映射，execute 阶段解引用。
//! - **Pass 添加顺序即执行顺序**：App 和 Plugin 决定渲染管线顺序，RenderGraph 不做拓扑重排。
//! - **自动 Barrier**：compile 阶段按线性 pass 序列跟踪 image 状态，自动计算 layout transition 和 memory barrier。

mod barrier;
mod executor;
mod export_info;
mod image_resource;
mod pass;
mod resource_handle;
mod resource_manager;
mod resource_state;
mod semaphore_info;

pub use executor::{CompiledGraph, RenderGraphBuilder};
pub use pass::{RgPass, RgPassBuilder, RgPassContext};
pub use resource_handle::RgImageHandle;
pub use resource_state::RgImageState;
pub use semaphore_info::RgSemaphoreInfo;
