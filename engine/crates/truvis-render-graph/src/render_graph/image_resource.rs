use ash::vk;

use truvis_render_interface::handles::{GfxImageHandle, GfxImageViewHandle};

use crate::render_graph::RgImageState;
use crate::render_graph::semaphore_info::RgSemaphoreInfo;

/// 图像资源的来源
#[derive(Clone, Debug)]
pub enum RgImageSource {
    /// 从外部导入的图像（已存在于 GfxResourceManager）
    Imported {
        image_handle: GfxImageHandle,
        view_handle: Option<GfxImageViewHandle>,
        /// 可选的外部 semaphore 等待（在首次使用此资源前等待）
        wait_semaphore: Option<RgSemaphoreInfo>,
    },
}

/// 图像资源条目
#[derive(Clone, Debug)]
pub struct RgImageResource {
    /// 资源来源
    pub source: RgImageSource,
    /// 当前状态
    pub current_state: RgImageState,
    /// 图像格式（用于推断 barrier aspect）
    pub format: vk::Format,
    /// 调试名称
    pub name: String,
}

// 创建与初始化
impl RgImageResource {
    /// 创建导入的图像资源
    ///
    /// # 参数
    /// - `name`: 资源调试名称
    /// - `image_handle`: 物理图像句柄
    /// - `view_handle`: 可选的图像视图句柄
    /// - `format`: 图像格式
    /// - `initial_state`: 图像的初始状态
    /// - `wait_semaphore`: 可选的外部 semaphore 等待信息
    pub fn imported(
        name: impl Into<String>,
        image_handle: GfxImageHandle,
        view_handle: Option<GfxImageViewHandle>,
        format: vk::Format,
        initial_state: RgImageState,
        wait_semaphore: Option<RgSemaphoreInfo>,
    ) -> Self {
        Self {
            source: RgImageSource::Imported {
                image_handle,
                view_handle,
                wait_semaphore,
            },
            current_state: initial_state,
            format,
            name: name.into(),
        }
    }
}

// 访问器
impl RgImageResource {
    /// 根据格式推断 aspect flags
    #[inline]
    pub fn infer_aspect(&self) -> vk::ImageAspectFlags {
        infer_aspect(self.format)
    }

    /// 获取物理 image handle（仅对导入资源有效）
    #[inline]
    pub fn physical_handle(&self) -> Option<GfxImageHandle> {
        match &self.source {
            RgImageSource::Imported { image_handle, .. } => Some(*image_handle),
        }
    }

    /// 获取物理 image view handle（仅对导入资源有效）
    #[inline]
    pub fn physical_view_handle(&self) -> Option<GfxImageViewHandle> {
        match &self.source {
            RgImageSource::Imported { view_handle, .. } => *view_handle,
        }
    }

    /// 获取等待的外部 semaphore（仅对导入资源有效）
    #[inline]
    pub fn wait_semaphore(&self) -> Option<RgSemaphoreInfo> {
        match &self.source {
            RgImageSource::Imported { wait_semaphore, .. } => *wait_semaphore,
        }
    }
}

/// 从格式推断 aspect，用于生成 image barrier。
fn infer_aspect(format: vk::Format) -> vk::ImageAspectFlags {
    match format {
        vk::Format::D16_UNORM | vk::Format::D32_SFLOAT | vk::Format::X8_D24_UNORM_PACK32 => vk::ImageAspectFlags::DEPTH,
        vk::Format::S8_UINT => vk::ImageAspectFlags::STENCIL,
        vk::Format::D16_UNORM_S8_UINT | vk::Format::D24_UNORM_S8_UINT | vk::Format::D32_SFLOAT_S8_UINT => {
            vk::ImageAspectFlags::DEPTH | vk::ImageAspectFlags::STENCIL
        }
        _ => vk::ImageAspectFlags::COLOR,
    }
}
