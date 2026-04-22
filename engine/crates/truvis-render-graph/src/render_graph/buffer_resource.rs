use ash::vk;

use truvis_render_interface::handles::GfxBufferHandle;

use crate::render_graph::RgBufferState;

/// 缓冲区资源描述（用于创建临时资源）
#[derive(Clone, Debug)]
pub struct RgBufferDesc {
    /// 缓冲区大小（字节）
    pub size: vk::DeviceSize,
    /// 缓冲区用途
    pub usage: vk::BufferUsageFlags,
}

impl Default for RgBufferDesc {
    fn default() -> Self {
        Self {
            size: 0,
            usage: vk::BufferUsageFlags::STORAGE_BUFFER,
        }
    }
}

// new & init
impl RgBufferDesc {
    /// 创建新描述
    #[inline]
    pub fn new(size: vk::DeviceSize, usage: vk::BufferUsageFlags) -> Self {
        Self { size, usage }
    }
}

/// 缓冲区资源的来源
#[derive(Clone, Debug)]
pub enum RgBufferSource {
    /// 从外部导入的缓冲区
    Imported { buffer_handle: GfxBufferHandle },
    /// 由 RenderGraph 创建的临时缓冲区
    Transient { desc: RgBufferDesc },
}

/// 缓冲区资源条目
#[derive(Clone, Debug)]
pub struct RgBufferResource {
    /// 资源来源
    pub source: RgBufferSource,
    /// 当前状态
    pub current_state: RgBufferState,
    /// 调试名称
    pub name: String,
    /// 当前版本
    pub version: u32,
}

// new & init
impl RgBufferResource {
    /// 创建导入的缓冲区资源
    pub fn imported(name: impl Into<String>, buffer_handle: GfxBufferHandle, initial_state: RgBufferState) -> Self {
        Self {
            source: RgBufferSource::Imported { buffer_handle },
            current_state: initial_state,
            name: name.into(),
            version: 0,
        }
    }

    /// 创建临时缓冲区资源
    pub fn transient(name: impl Into<String>, desc: RgBufferDesc) -> Self {
        Self {
            source: RgBufferSource::Transient { desc },
            current_state: RgBufferState::UNDEFINED,
            name: name.into(),
            version: 0,
        }
    }
}

// getter
impl RgBufferResource {
    /// 获取物理 buffer handle（仅对导入资源有效）
    #[inline]
    pub fn physical_handle(&self) -> Option<GfxBufferHandle> {
        match &self.source {
            RgBufferSource::Imported { buffer_handle } => Some(*buffer_handle),
            RgBufferSource::Transient { .. } => None,
        }
    }
}
