//! 早期渲染子系统接口。
//!
//! 当前 runtime 主路径已经收敛到 `RenderRuntime` lifecycle Ctx 与上层 plugin 编排；
//! 该模块保留给仍需要按帧接入 runtime 前置阶段的轻量扩展点。

pub mod subsystem;
