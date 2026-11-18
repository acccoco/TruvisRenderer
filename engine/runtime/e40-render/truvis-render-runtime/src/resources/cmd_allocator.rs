use ash::vk;

use truvis_gfx::{
    commands::{command_buffer::GfxCommandBuffer, command_pool::GfxCommandPool},
    gfx::{GfxDeviceCtx, GfxDeviceInfoCtx},
};

use truvis_render_foundation::frame_label::FrameLabel;

/// 命令缓冲分配器
///
/// 每个 FIF slot 持有独立的 TRANSIENT pool；Runtime 等待该 slot 的 timeline 完成后
/// reset pool，保留已分配的 command handles 供重新录制。显式 free 或 destroy 才释放它们。
/// 返回的 command clone 只是同一 native 对象的别名，调用方不能绕过该 owner 单独释放。
pub struct CmdAllocator {
    /// 为每个 frame 分配一个 command pool
    graphics_command_pools: [GfxCommandPool; FrameLabel::COUNT],

    /// 每个 command pool 已经分配出去的 command buffer，用于集中 free
    /// 或其他操作
    allocated_command_buffers: [Vec<GfxCommandBuffer>; FrameLabel::COUNT],

    destroyed: bool,
}

impl CmdAllocator {
    pub fn new(device_ctx: GfxDeviceCtx<'_>, device_info_ctx: GfxDeviceInfoCtx<'_>) -> Self {
        let _span = tracy_client::span!("CmdAllocator::new");

        let graphics_command_pools = {
            let _span = tracy_client::span!("CmdAllocator::new/graphics_command_pools");
            FrameLabel::ALL.map(|label| {
                GfxCommandPool::new(
                    device_ctx,
                    device_info_ctx.gfx_queue_family(),
                    vk::CommandPoolCreateFlags::TRANSIENT,
                    &format!("render_context_graphics_command_pool_{}", label),
                )
            })
        };
        let allocated_command_buffers = FrameLabel::ALL.map(|_| Vec::new());

        Self {
            graphics_command_pools,
            allocated_command_buffers,
            destroyed: false,
        }
    }
}
impl Drop for CmdAllocator {
    fn drop(&mut self) {
        debug_assert!(self.destroyed, "CmdAllocator dropped without explicit destroy");
    }
}
// 销毁
impl CmdAllocator {
    pub fn destroy(mut self, ctx: GfxDeviceCtx<'_>) {
        self.destroy_mut(ctx);
    }

    fn destroy_mut(&mut self, ctx: GfxDeviceCtx<'_>) {
        if self.destroyed {
            return;
        }

        for frame_label in 0..FrameLabel::COUNT {
            let commands = std::mem::take(&mut self.allocated_command_buffers[frame_label]);
            if !commands.is_empty() {
                self.graphics_command_pools[frame_label].free_command_buffers(ctx, commands);
            }
            self.graphics_command_pools[frame_label].destroy(ctx);
        }

        self.destroyed = true;
    }
}
// 工具函数
impl CmdAllocator {
    /// 分配属于指定 FIF slot 的 command 别名；可在该 slot 后续安全复用时重新录制。
    pub fn alloc_command_buffer(
        &mut self,
        ctx: GfxDeviceCtx<'_>,
        frame_label: FrameLabel,
        debug_name: &str,
    ) -> GfxCommandBuffer {
        let name = format!("[{}]{}", frame_label, debug_name);
        let cmd = GfxCommandBuffer::new(ctx, &self.graphics_command_pools[*frame_label], &name);

        self.allocated_command_buffers[*frame_label].push(cmd.clone());
        cmd
    }

    /// 调用前必须等待该 FIF slot 的 GPU 使用完成；保留 handle，只重置录制状态。
    pub fn reset_frame_commands(&mut self, ctx: GfxDeviceCtx<'_>, frame_label: FrameLabel) {
        let _span = tracy_client::span!("reset_frame_commands");

        self.graphics_command_pools[*frame_label].reset_command_pool(ctx);
    }

    /// 调用前必须等待该 FIF slot 的 GPU 使用完成，并撤销所有外部 command clone。
    /// 释放后旧 handle 无法再使用；Runtime 日常帧复用使用 reset，不调用此入口。
    pub fn free_frame_commands(&mut self, ctx: GfxDeviceCtx<'_>, frame_label: FrameLabel) {
        let _span = tracy_client::span!("free_frame_commands");

        self.free_frame_commands_internal(ctx, *frame_label);
    }

    fn free_frame_commands_internal(&mut self, ctx: GfxDeviceCtx<'_>, frame_label: usize) {
        // 释放当前 frame 的 command buffer 的资源
        let gc_cmds = std::mem::take(&mut self.allocated_command_buffers[frame_label]);
        if !gc_cmds.is_empty() {
            self.graphics_command_pools[frame_label].free_command_buffers(ctx, gc_cmds);
        }

        // 这个调用并不会释放资源，而是将 pool 内的 command buffer 设置到初始状态
        self.graphics_command_pools[frame_label].reset_command_pool(ctx);
    }
}
