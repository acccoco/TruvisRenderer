//! 早期渲染子系统接口。
//!
//! 当前 backend 主路径已经收敛到 `RenderBackend` lifecycle Ctx 与上层 plugin 编排；
//! 该模块保留给仍需要按帧接入 backend 前置阶段的轻量扩展点。

pub mod subsystem;
