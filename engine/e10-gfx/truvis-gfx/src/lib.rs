//! Vulkan RHI (Rendering Hardware Interface) 抽象层
//!
//! 提供对 Vulkan API 的高层封装，包括设备管理、命令缓冲、描述符、管线等核心功能。
//! [`Gfx`](gfx::Gfx) 是显式 root owner，由上层持有并通过 typed Ctx 向资源创建、
//! 提交和销毁路径传递最小依赖。

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

pub use gfx_core::VulkanEntrySource;
