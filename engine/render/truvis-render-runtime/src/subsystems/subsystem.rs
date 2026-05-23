/// 早期渲染子系统的最小帧前扩展点。
///
/// 新代码通常应通过上层 plugin 和 `RenderRuntime*Ctx` 接入生命周期；该 trait 只表达
/// “render 前执行一次”的窄契约，不拥有 runtime 状态，也不负责 command submit。
pub trait Subsystem {
    /// 在 render 阶段前执行子系统自己的准备工作。
    fn before_render(&mut self);
}
