/// 表示一个渲染子系统
pub trait Subsystem {
    fn before_render(&mut self);
}
