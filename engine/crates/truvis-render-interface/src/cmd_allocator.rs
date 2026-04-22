use ash::vk;

use truvis_gfx::{
    commands::{command_buffer::GfxCommandBuffer, command_pool::GfxCommandPool},
    gfx::Gfx,
};

use crate::frame_counter::FrameCounter;
use crate::pipeline_settings::FrameLabel;

/// 命令缓冲分配器
///
/// 为每帧管理独立的命令池和命令缓冲，支持帧内批量分配和帧间自动回收。
/// 采用 TRANSIENT 标志优化临时命令的分配性能。
///
/// # Frames in Flight
/// - 每帧独立的 CommandPool（避免同步冲突）
/// - 帧结束时统一释放命令缓冲
/// - 命令缓冲自动添加帧标签：`[F42A]my-pass`
pub struct CmdAllocator {
    /// 为每个 frame 分配一个 command pool
    graphics_command_pools: [GfxCommandPool; FrameCounter::fif_count()],

    /// 每个 command pool 已经分配出去的 command buffer，用于集中 free
    /// 或其他操作
    allocated_command_buffers: [Vec<GfxCommandBuffer>; FrameCounter::fif_count()],
}

// new & init
impl Default for CmdAllocator {
    fn default() -> Self {
        Self::new()
    }
}

impl CmdAllocator {
    pub fn new() -> Self {
        let graphics_command_pools = FrameCounter::frame_labes().map(|i| {
            GfxCommandPool::new(
                Gfx::get().gfx_queue_family(),
                vk::CommandPoolCreateFlags::TRANSIENT,
                &format!("render_context_graphics_command_pool_{}", i),
            )
        });
        let allocated_command_buffers = FrameCounter::frame_labes().map(|_| Vec::new());

        Self {
            graphics_command_pools,
            allocated_command_buffers,
        }
    }
}
impl Drop for CmdAllocator {
    fn drop(&mut self) {
        log::info!("Dropping CmdAllocator and destroying command pools.");
        for pool in &mut self.graphics_command_pools {
            pool.destroy()
        }
    }
}
// destroy
impl CmdAllocator {
    pub fn destroy(self) {}
}
// tools
impl CmdAllocator {
    /// 分配 command buffer，在当前 frame 使用
    pub fn alloc_command_buffer(&mut self, frame_label: FrameLabel, debug_name: &str) -> GfxCommandBuffer {
        let name = format!("[{}]{}", frame_label, debug_name);
        let cmd = GfxCommandBuffer::new(&self.graphics_command_pools[*frame_label], &name);

        self.allocated_command_buffers[*frame_label].push(cmd.clone());
        cmd
    }

    /// 重置当前 frame 的 command buffers，这些 command buffers 可以重新录制
    pub fn reset_frame_commands(&mut self, frame_label: FrameLabel) {
        let _span = tracy_client::span!("reset_frame_commands");

        self.graphics_command_pools[*frame_label].reset_command_pool();
    }

    /// 释放当前 frame 的 command buffers，这些 commands 无法再使用
    pub fn free_frame_commands(&mut self, frame_label: FrameLabel) {
        let _span = tracy_client::span!("free_frame_commands");

        self.free_frame_commands_internal(*frame_label);
    }

    fn free_frame_commands_internal(&mut self, frame_label: usize) {
        // 释放当前 frame 的 command buffer 的资源
        let gc_cmds = std::mem::take(&mut self.allocated_command_buffers[frame_label]);
        if !gc_cmds.is_empty() {
            self.graphics_command_pools[frame_label].free_command_buffers(gc_cmds);
        }

        // 这个调用并不会释放资源，而是将 pool 内的 command buffer 设置到初始状态
        self.graphics_command_pools[frame_label].reset_command_pool();
    }
}
