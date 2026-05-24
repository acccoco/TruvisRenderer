//! runtime 自带的轻量平台辅助类型。
//!
//! 这里不封装窗口事件或输入系统；窗口生命周期由 `truvis-winit-app`/frame runtime 提供，
//! runtime 只保留 prepare 阶段需要读取的默认相机数据。

pub mod camera;
