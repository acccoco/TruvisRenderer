//! Shared render pass implementations.
//!
//! Contains reusable GPU passes: real-time ray tracing, accumulation,
//! denoising, tone-mapping (SDR), blit, resolve, and Phong shading.
//!
//! Pipeline orchestration and GUI RenderGraph integration remain in `truvis-app`
//! where concrete apps compose plugins and decide pass order.

pub mod accum_pass;
pub mod blit_pass;
pub mod denoise_accum_pass;
pub mod phong_pass;
pub mod realtime_rt_pass;
pub mod resolve_pass;
pub mod sdr_pass;
