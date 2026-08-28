//! Renderer 层共享 render pass 实现。
//!
//! 包含可复用的 GPU pass：real-time ray tracing、accumulation、
//! tone-mapping (SDR)、image clear、resolve、coordinate gizmo 和 Phong shading。
//!
//! 本 crate 表达 Truvis Renderer / samples 复用的具体渲染效果，不属于
//! engine core。渲染子系统编排、ImGui 集成和业务效果顺序由各自 Renderer capability crate 持有。

mod compute_pass;
pub mod effects;
pub mod post_process;
pub mod ray_tracing;
pub(crate) mod streamline_pass;
