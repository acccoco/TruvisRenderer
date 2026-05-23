//! 共享 render pass 实现。
//!
//! 包含可复用的 GPU pass：real-time ray tracing、accumulation、
//! denoising、tone-mapping (SDR)、blit、resolve 和 Phong shading。
//!
//! Pipeline 编排和 GUI RenderGraph 集成保留在 `truvis-app/app-kit`
//! 及具体 app 中，由具体 app 组合 plugin 并决定 pass 顺序。

pub mod accum_pass;
pub mod blit_pass;
pub mod denoise_accum_pass;
pub mod phong_pass;
pub mod realtime_rt_pass;
pub mod resolve_pass;
pub mod sdr_pass;
