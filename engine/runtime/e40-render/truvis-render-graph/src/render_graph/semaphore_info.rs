use ash::vk;

/// 借用的提交同步声明；Some(value) 表示 timeline，None 表示 binary。
/// raw handle 的实际类型与生命周期由调用方保证，graph 只校验声明间的一致性。
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
    /// 同一 native semaphore 的 wait 下限合并，等待最大值满足全部资源的完成要求。
    pub(crate) fn merge_wait(&mut self, other: Self) {
        assert_eq!(self.semaphore, other.semaphore);
        assert_eq!(self.value.is_some(), other.value.is_some(), "conflicting semaphore types");
        self.value = self.value.zip(other.value).map(|(left, right)| left.max(right));
        self.stage |= other.stage;
    }

    /// 一个 batch 只能承诺一个 signal 值；重复声明可扩展完成范围，不能产生多个里程碑。
    pub(crate) fn merge_signal(&mut self, other: Self) {
        assert_eq!(self.semaphore, other.semaphore);
        assert_eq!(self.value, other.value, "conflicting semaphore signal types or values");
        self.stage |= other.stage;
    }

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
