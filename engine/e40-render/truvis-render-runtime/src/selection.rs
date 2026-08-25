//! World 语义的选择渲染接口。
//!
//! 本模块只暴露 CPU `World` 层可以理解的选择描述：`InstanceHandle + submesh_index`。
//! GPU instance slot、draw cache 和 ready gate 都留在 runtime 内部解析，避免 App 或 pass
//! 依赖 RenderWorld 的私有数据结构。

use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_render_foundation::frame_counter::FrameLabel;
use truvis_world::guid_new_type::InstanceHandle;

/// CPU `World` 语义下的单个 submesh 选择。
///
/// `instance` 是 `World` 暴露给 App 的运行时实例 handle，`submesh_index` 是该实例引用
/// mesh 的 instance-local submesh 序号。该类型不承诺 GPU ready；runtime 会在 render
/// 阶段根据当前 prepare 快照决定是否真的存在可绘制 draw。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorldSubmeshSelection {
    pub instance: InstanceHandle,
    pub submesh_index: u32,
}

/// 选中 submesh 的只读光栅化录制接口。
///
/// 该 trait 由 runtime 私有 `RenderWorld` 实现，App/pass 只能提交 `WorldSubmeshSelection`。
/// pending、已删除、未 GPU-ready、slot 快照过期或 submesh 越界都会返回 `false` 并跳过绘制，
/// 调用方不得把 `false` 当作错误处理。
pub trait WorldSubmeshRasterView {
    fn draw_selected_submesh_raster(
        &self,
        frame_label: FrameLabel,
        cmd: &GfxCommandBuffer,
        selection: WorldSubmeshSelection,
        before_draw: &mut dyn FnMut(u32, u32),
    ) -> bool;
}
