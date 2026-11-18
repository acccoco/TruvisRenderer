use truvis_gfx::resources::image::GfxImage;
use truvis_gfx::resources::image_view::GfxImageView;

use crate::handles::{GfxImageHandle, GfxImageViewHandle};

/// GPU 资源只读查询契约。
///
/// RenderGraph 只需要在录制 barrier 和 pass 时把稳定句柄解析为底层
/// image / image view；具体资源生命周期 owner 由 runtime 持有并实现该契约。
pub trait GfxResourceAccess {
    fn get_image(&self, handle: GfxImageHandle) -> Option<&GfxImage>;

    fn get_image_view(&self, handle: GfxImageViewHandle) -> Option<&GfxImageView>;
}
