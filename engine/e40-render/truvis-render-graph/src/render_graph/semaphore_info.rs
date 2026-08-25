use ash::vk;

// TODO RgSemaphoreInfo 可以考虑提升到 Gfx 里面去
#[derive(Clone, Copy, Debug)]
pub struct RgSemaphoreInfo {
    /// Vulkan semaphore 原始句柄
    pub semaphore: vk::Semaphore,
    /// 等待的 pipeline stage
    pub stage: vk::PipelineStageFlags2,
    /// Timeline semaphore 的等待值（binary semaphore 为 None）
    pub value: Option<u64>,
}

impl RgSemaphoreInfo {
    /// 创建 binary semaphore
    #[inline]
    pub fn binary(semaphore: vk::Semaphore, stage: vk::PipelineStageFlags2) -> Self {
        Self {
            semaphore,
            stage,
            value: None,
        }
    }

    /// 创建 timeline semaphore
    #[inline]
    pub fn timeline(semaphore: vk::Semaphore, stage: vk::PipelineStageFlags2, value: u64) -> Self {
        Self {
            semaphore,
            stage,
            value: Some(value),
        }
    }
}
