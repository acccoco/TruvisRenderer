use ash::vk;
use image::GenericImageView;

use crate::asset_loader::{LoadResult, TextureLoadRequest};
use crate::handle::LoadedTextureBytes;

/// 实际的纹理加载任务，运行在 Rayon 线程池中。
///
/// 执行顺序是文件读取 -> image crate 解码 -> 统一转换为 RGBA8 upload-ready bytes。
/// 这里不创建 Vulkan image，返回的 `LoadedTextureBytes` 只用于后续 render-side 上传。
pub(crate) fn load_texture_task(req: TextureLoadRequest) -> LoadResult {
    let _span = tracy_client::span!("load_texture_task");
    log::info!("Loading texture: {:?}", req.path);

    let img_result = image::open(&req.path);

    match img_result {
        Ok(img) => {
            let (width, height) = img.dimensions();
            // asset 层统一输出 RGBA8，减少 render-side uploader 的格式分支。
            let img = img.into_rgba8();
            let pixels = img.into_raw();

            let data = LoadedTextureBytes {
                pixels,
                extent: vk::Extent3D {
                    width,
                    height,
                    depth: 1,
                },
                format: vk::Format::R8G8B8A8_UNORM,
            };

            LoadResult::TextureSuccess {
                handle: req.handle,
                data,
            }
        }
        Err(e) => {
            log::error!("Failed to load texture {:?}: {}", req.path, e);
            LoadResult::TextureFailure(req.handle, e.to_string())
        }
    }
}
