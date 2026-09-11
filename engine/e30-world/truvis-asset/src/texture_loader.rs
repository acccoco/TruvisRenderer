use std::sync::Arc;

use ash::vk;
use half::f16;
use image::{DynamicImage, GenericImageView};

use crate::asset_loader::{LoadResult, TextureLoadRequest};
use crate::handle::{TextureBytes, TextureLoadDesc, TexturePixels};

/// 实际的纹理加载任务，运行在 Rayon 线程池中。
///
/// 执行顺序是文件读取 -> image crate 解码 -> 转换为 upload-ready RGBA8/RGBA16F。
/// 这里不创建 Vulkan image，返回的 `TextureBytes` 只用于后续 render-side 上传。
pub(crate) fn load_texture_task(req: TextureLoadRequest) -> LoadResult {
    let _span = tracy_client::span!("load_texture_task");
    log::info!("Loading texture: {}", req.desc.source_label());

    let img_result = match &req.desc {
        TextureLoadDesc::File { path } => image::open(path),
        TextureLoadDesc::Embedded { bytes, .. } => image::load_from_memory(bytes),
    };

    match img_result {
        Ok(img) => {
            let (width, height) = img.dimensions();
            let extent = vk::Extent3D {
                width,
                height,
                depth: 1,
            };
            let pixels = match img {
                // Radiance HDR 与 OpenEXR 解码结果必须在这里、仍为 f32 时截获；
                // 调用 `into_rgba8` 会把所有大于 1.0 的 radiance 永久量化掉。
                DynamicImage::ImageRgb32F(img) => TexturePixels::Rgba16Float(
                    img.pixels()
                        .flat_map(|pixel| {
                            let [r, g, b] = pixel.0;
                            [
                                hdr_channel_to_f16_bits(r),
                                hdr_channel_to_f16_bits(g),
                                hdr_channel_to_f16_bits(b),
                                f16::ONE.to_bits(),
                            ]
                        })
                        .collect::<Vec<_>>()
                        .into(),
                ),
                DynamicImage::ImageRgba32F(img) => TexturePixels::Rgba16Float(
                    img.pixels()
                        .flat_map(|pixel| {
                            let [r, g, b, a] = pixel.0;
                            [
                                hdr_channel_to_f16_bits(r),
                                hdr_channel_to_f16_bits(g),
                                hdr_channel_to_f16_bits(b),
                                hdr_alpha_to_f16_bits(a),
                            ]
                        })
                        .collect::<Vec<_>>()
                        .into(),
                ),
                img => TexturePixels::Rgba8(Arc::from(img.into_rgba8().into_raw())),
            };

            match TextureBytes::new(pixels, extent) {
                Ok(data) => LoadResult::TextureSuccess {
                    handle: req.handle,
                    data,
                },
                Err(error) => {
                    log::error!("Decoded texture {} has invalid payload: {}", req.desc.source_label(), error);
                    LoadResult::TextureFailure(req.handle, error)
                }
            }
        }
        Err(error) => {
            log::error!("Failed to load texture {}: {}", req.desc.source_label(), error);
            LoadResult::TextureFailure(req.handle, error.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use image::{ColorType, ImageEncoder};
    use slotmap::Key;

    use super::*;
    use crate::asset_loader::TextureLoadRequest;
    use crate::handle::{EmbeddedTextureId, TextureLoadDesc, TextureLoadHandle};

    #[test]
    fn embedded_rgba_payload_decodes_to_upload_ready_bytes() {
        let mut encoded = Vec::new();
        image::codecs::png::PngEncoder::new(&mut encoded)
            .write_image(&[255, 0, 0, 255], 1, 1, ColorType::Rgba8.into())
            .expect("encode test png");

        let result = load_texture_task(TextureLoadRequest {
            handle: TextureLoadHandle::null(),
            desc: TextureLoadDesc::Embedded {
                identity: EmbeddedTextureId { image_index: 0 },
                bytes: Arc::from(encoded),
                mime_type: Some("image/png".to_string()),
            },
        });

        let LoadResult::TextureSuccess { data, .. } = result else {
            panic!("embedded texture should decode successfully");
        };
        assert_eq!(data.extent().width, 1);
        assert_eq!(data.extent().height, 1);
        assert_eq!(data.format(), vk::Format::R8G8B8A8_UNORM);
        assert_eq!(data.texel_count(), 1);
        assert_eq!(data.as_bytes().len(), 4);
    }
}

/// 把 scene-linear HDR channel 规范化到 binary16 可表示范围。
///
/// 负 radiance 与非有限值都不能参与天空能量分布；这里将其清为 0，同时保留大于
/// 1.0 的有限 HDR 数值。65504 是 IEEE-754 binary16 最大有限值。
#[inline]
fn hdr_channel_to_f16_bits(value: f32) -> u16 {
    let value = if value.is_finite() { value.clamp(0.0, f16::MAX.to_f32()) } else { 0.0 };
    f16::from_f32(value).to_bits()
}

/// alpha 不表示辐亮度，只允许 [0, 1]；无效 alpha 按 opaque 处理。
#[inline]
fn hdr_alpha_to_f16_bits(value: f32) -> u16 {
    let value = if value.is_finite() { value.clamp(0.0, 1.0) } else { 1.0 };
    f16::from_f32(value).to_bits()
}
