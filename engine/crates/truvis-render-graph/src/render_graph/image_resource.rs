use ash::vk;

use truvis_gfx::resources::image_view::GfxImageViewDesc;
use truvis_render_interface::handles::{GfxImageHandle, GfxImageViewHandle};

use crate::render_graph::RgImageState;
use crate::render_graph::semaphore_info::RgSemaphoreInfo;

/// 图像资源描述（用于创建临时资源）
///
/// 包含创建 `vk::Image` 所需的所有信息，以及可选的默认视图描述。
#[derive(Clone, Debug)]
pub struct RgImageDesc {
    /// 图像宽度
    pub width: u32,
    /// 图像高度
    pub height: u32,
    /// 图像深度（3D 纹理）
    pub depth: u32,
    /// Mip 级别数
    pub mip_levels: u32,
    /// 数组层数
    pub array_layers: u32,
    /// 图像格式
    pub format: vk::Format,
    /// 图像用途
    pub usage: vk::ImageUsageFlags,
    /// 采样数
    pub samples: vk::SampleCountFlags,
    /// 图像类型
    pub image_type: vk::ImageType,
    // TODO remove me RG pass 并不关心是 image 还是 image view，只关心使用
    /// 可选的默认视图描述（用于自动创建 ImageView）
    pub default_view_desc: Option<GfxImageViewDesc>,
}

impl Default for RgImageDesc {
    fn default() -> Self {
        Self {
            width: 1,
            height: 1,
            depth: 1,
            mip_levels: 1,
            array_layers: 1,
            format: vk::Format::R8G8B8A8_UNORM,
            usage: vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::STORAGE,
            samples: vk::SampleCountFlags::TYPE_1,
            image_type: vk::ImageType::TYPE_2D,
            default_view_desc: None,
        }
    }
}

// new & init & builder
impl RgImageDesc {
    /// 创建 2D 图像描述
    #[inline]
    pub fn new_2d(width: u32, height: u32, format: vk::Format, usage: vk::ImageUsageFlags) -> Self {
        Self {
            width,
            height,
            format,
            usage,
            ..Default::default()
        }
    }

    /// 设置用途（链式调用）
    #[inline]
    pub fn with_usage(mut self, usage: vk::ImageUsageFlags) -> Self {
        self.usage = usage;
        self
    }

    /// 设置默认视图描述（链式调用）
    #[inline]
    pub fn with_default_view(mut self, view_desc: GfxImageViewDesc) -> Self {
        self.default_view_desc = Some(view_desc);
        self
    }

    /// 自动推断并生成默认视图描述
    ///
    /// 根据图像格式和类型推断 aspect 和 view_type
    pub fn infer_default_view(&self) -> GfxImageViewDesc {
        let aspect = Self::infer_aspect(self.format);
        let view_type = Self::infer_view_type(self.image_type, self.array_layers);

        GfxImageViewDesc::new(self.format, view_type, aspect, (0, self.mip_levels as u8), (0, self.array_layers as u8))
    }

    /// 从格式推断 aspect
    pub fn infer_aspect(format: vk::Format) -> vk::ImageAspectFlags {
        match format {
            vk::Format::D16_UNORM | vk::Format::D32_SFLOAT | vk::Format::X8_D24_UNORM_PACK32 => {
                vk::ImageAspectFlags::DEPTH
            }
            vk::Format::S8_UINT => vk::ImageAspectFlags::STENCIL,
            vk::Format::D16_UNORM_S8_UINT | vk::Format::D24_UNORM_S8_UINT | vk::Format::D32_SFLOAT_S8_UINT => {
                vk::ImageAspectFlags::DEPTH | vk::ImageAspectFlags::STENCIL
            }
            _ => vk::ImageAspectFlags::COLOR,
        }
    }

    /// 从图像类型推断视图类型
    fn infer_view_type(image_type: vk::ImageType, array_layers: u32) -> vk::ImageViewType {
        match image_type {
            vk::ImageType::TYPE_1D => {
                if array_layers > 1 {
                    vk::ImageViewType::TYPE_1D_ARRAY
                } else {
                    vk::ImageViewType::TYPE_1D
                }
            }
            vk::ImageType::TYPE_2D => {
                if array_layers > 1 {
                    vk::ImageViewType::TYPE_2D_ARRAY
                } else {
                    vk::ImageViewType::TYPE_2D
                }
            }
            vk::ImageType::TYPE_3D => vk::ImageViewType::TYPE_3D,
            _ => vk::ImageViewType::TYPE_2D,
        }
    }
}

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
    /// 由 RenderGraph 创建的临时图像
    Transient { desc: RgImageDesc },
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
    /// 当前版本（被写入的次数）
    pub version: u32,
}

// new & init
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
            version: 0,
        }
    }

    /// 创建临时图像资源
    pub fn transient(name: impl Into<String>, desc: RgImageDesc) -> Self {
        let format = desc.format;
        Self {
            source: RgImageSource::Transient { desc },
            current_state: RgImageState::UNDEFINED_TOP,
            format,
            name: name.into(),
            version: 0,
        }
    }
}

// getters
impl RgImageResource {
    /// 根据格式推断 aspect flags
    #[inline]
    pub fn infer_aspect(&self) -> vk::ImageAspectFlags {
        RgImageDesc::infer_aspect(self.format)
    }

    /// 获取物理 image handle（仅对导入资源有效）
    #[inline]
    pub fn physical_handle(&self) -> Option<GfxImageHandle> {
        match &self.source {
            RgImageSource::Imported { image_handle, .. } => Some(*image_handle),
            RgImageSource::Transient { .. } => None,
        }
    }

    /// 获取物理 image view handle（仅对导入资源有效）
    #[inline]
    pub fn physical_view_handle(&self) -> Option<GfxImageViewHandle> {
        match &self.source {
            RgImageSource::Imported { view_handle, .. } => *view_handle,
            RgImageSource::Transient { .. } => None,
        }
    }

    /// 获取等待的外部 semaphore（仅对导入资源有效）
    #[inline]
    pub fn wait_semaphore(&self) -> Option<RgSemaphoreInfo> {
        match &self.source {
            RgImageSource::Imported { wait_semaphore, .. } => *wait_semaphore,
            RgImageSource::Transient { .. } => None,
        }
    }
}
