//! Vulkan RHI (Rendering Hardware Interface) 抽象层
//!
//! 提供对 Vulkan API 的高层封装，包括设备管理、命令缓冲、描述符、管线等核心功能。
//! 所有 Vulkan 资源通过 [`Gfx`] 单例统一管理，简化生命周期和借用关系。

pub mod basic;
pub mod commands;
pub mod descriptors;
pub mod foundation;
pub mod gfx;
pub mod gfx_core;
pub mod pipelines;
pub mod query;
pub mod raytracing;
pub mod resources;
pub mod sampler;
pub mod swapchain;
pub mod utilities;
