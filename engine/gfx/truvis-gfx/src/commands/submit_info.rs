use ash::vk;
use itertools::Itertools;

use crate::commands::{command_buffer::GfxCommandBuffer, semaphore::GfxSemaphore};

/// Gfx 关于 submitInfo 的封装，更易用
#[derive(Default)]
pub struct GfxSubmitInfo {
    inner: vk::SubmitInfo2<'static>,

    _command_buffers: Vec<vk::CommandBufferSubmitInfo<'static>>,
    wait_infos: Vec<vk::SemaphoreSubmitInfo<'static>>,
    signal_infos: Vec<vk::SemaphoreSubmitInfo<'static>>,
}

impl GfxSubmitInfo {
    pub fn new(commands: &[GfxCommandBuffer]) -> Self {
        let command_buffers = commands
            .iter()
            .map(|cmd| vk::CommandBufferSubmitInfo::default().command_buffer(cmd.vk_handle()))
            .collect_vec();

        let inner = vk::SubmitInfo2 {
            // 暂时不使用该 flag
            flags: vk::SubmitFlags::empty(),

            command_buffer_info_count: command_buffers.len() as u32,
            p_command_buffer_infos: command_buffers.as_ptr(),
            ..Default::default()
        };

        Self {
            inner,
            _command_buffers: command_buffers,
            wait_infos: vec![],
            signal_infos: vec![],
        }
    }

    #[inline]
    pub fn submit_info(&self) -> vk::SubmitInfo2<'_> {
        self.inner
            .command_buffer_infos(&self._command_buffers)
            .wait_semaphore_infos(&self.wait_infos)
            .signal_semaphore_infos(&self.signal_infos)
    }

    #[inline]
    pub fn wait(mut self, semaphore: &GfxSemaphore, stage: vk::PipelineStageFlags2, value: Option<u64>) -> Self {
        self.wait_infos.push(
            vk::SemaphoreSubmitInfo::default()
                .semaphore(semaphore.handle())
                .stage_mask(stage)
                .value(value.unwrap_or_default()),
        );
        self
    }

    /// 使用原始 Vulkan semaphore 句柄添加 wait 信号
    ///
    /// 这个方法直接接收 `vk::Semaphore` 句柄，适用于从外部传入的 semaphore。
    #[inline]
    pub fn wait_raw(mut self, semaphore: vk::Semaphore, stage: vk::PipelineStageFlags2, value: Option<u64>) -> Self {
        self.wait_infos.push(
            vk::SemaphoreSubmitInfo::default().semaphore(semaphore).stage_mask(stage).value(value.unwrap_or_default()),
        );
        self
    }

    #[inline]
    pub fn signal(mut self, semaphore: &GfxSemaphore, stage: vk::PipelineStageFlags2, value: Option<u64>) -> Self {
        self.signal_infos.push(
            vk::SemaphoreSubmitInfo::default()
                .semaphore(semaphore.handle())
                .stage_mask(stage)
                .value(value.unwrap_or_default()),
        );
        self
    }

    /// 使用原始 Vulkan semaphore 句柄添加 signal 信号
    ///
    /// 这个方法直接接收 `vk::Semaphore` 句柄，适用于从外部传入的 semaphore。
    #[inline]
    pub fn signal_raw(mut self, semaphore: vk::Semaphore, stage: vk::PipelineStageFlags2, value: Option<u64>) -> Self {
        self.signal_infos.push(
            vk::SemaphoreSubmitInfo::default().semaphore(semaphore).stage_mask(stage).value(value.unwrap_or_default()),
        );
        self
    }
}
