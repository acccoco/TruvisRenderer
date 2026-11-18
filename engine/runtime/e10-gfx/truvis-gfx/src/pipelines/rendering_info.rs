use ash::vk;

pub struct GfxRenderingInfo {
    color_attach_info: Vec<vk::RenderingAttachmentInfo<'static>>,
    depth_attach_info: Option<vk::RenderingAttachmentInfo<'static>>,
    range: vk::Rect2D,
}
impl GfxRenderingInfo {
    pub fn new(
        color_attach_image: Vec<vk::ImageView>,
        depth_attach_image: Option<vk::ImageView>,
        range: vk::Rect2D,
    ) -> Self {
        Self {
            color_attach_info: color_attach_image.iter().map(|view| Self::get_color_attachment(*view)).collect(),
            depth_attach_info: depth_attach_image.map(Self::get_depth_attachment),
            range,
        }
    }

    pub fn rendering_info(&self) -> vk::RenderingInfo<'_> {
        let mut info = vk::RenderingInfo::default()
            .layer_count(1)
            .render_area(self.range)
            .color_attachments(&self.color_attach_info);
        if let Some(depth_attach) = &self.depth_attach_info {
            info = info.depth_attachment(depth_attach)
        }
        info
    }

    fn get_color_attachment(image_view: vk::ImageView) -> vk::RenderingAttachmentInfo<'static> {
        vk::RenderingAttachmentInfo::default()
            .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            .image_view(image_view)
            .load_op(vk::AttachmentLoadOp::CLEAR)
            .store_op(vk::AttachmentStoreOp::STORE)
            .clear_value(vk::ClearValue {
                color: vk::ClearColorValue {
                    float32: [0_f32, 0_f32, 0_f32, 1_f32],
                },
            })
    }

    fn get_depth_attachment(depth_image_view: vk::ImageView) -> vk::RenderingAttachmentInfo<'static> {
        vk::RenderingAttachmentInfo::default()
            .image_layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL)
            .image_view(depth_image_view)
            .load_op(vk::AttachmentLoadOp::CLEAR)
            .store_op(vk::AttachmentStoreOp::STORE)
            .clear_value(vk::ClearValue {
                depth_stencil: vk::ClearDepthStencilValue {
                    depth: 1_f32, // 1 表示无限远
                    stencil: 0,
                },
            })
    }
}
