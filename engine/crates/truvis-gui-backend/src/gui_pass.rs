use crate::gui_mesh::GuiMesh;
use crate::gui_vertex_layout::ImGuiVertexLayoutAoS;
use ash::vk;
use imgui::TextureId;
use itertools::Itertools;
use std::collections::HashMap;
use std::rc::Rc;
use truvis_gfx::basic::bytes::BytesConvert;
use truvis_gfx::resources::layout::GfxVertexLayout;
use truvis_gfx::{
    commands::command_buffer::GfxCommandBuffer,
    pipelines::{
        graphics_pipeline::{GfxGraphicsPipeline, GfxGraphicsPipelineCreateInfo, GfxPipelineLayout},
        shader::GfxShaderStageInfo,
    },
};
use truvis_path::TruvisPath;
use truvis_render_interface::bindless_manager::BindlessManager;
use truvis_render_interface::global_descriptor_sets::GlobalDescriptorSets;
use truvis_render_interface::handles::GfxImageViewHandle;
use truvis_render_interface::pipeline_settings::FrameLabel;
use truvis_shader_binding::gpu;
use truvis_shader_binding::gpu::SrvHandle;
use truvis_utils::count_indexed_array;
use truvis_utils::enumed_map;

enumed_map!(ShaderStage<GfxShaderStageInfo>: {
    Vertex: GfxShaderStageInfo {
        stage: vk::ShaderStageFlags::VERTEX,
        entry_point: c"vsmain",
        path: TruvisPath::shader_build_path_str("imgui/imgui.slang"),
    },
    Fragment: GfxShaderStageInfo {
        stage: vk::ShaderStageFlags::FRAGMENT,
        entry_point: c"psmain",
        path: TruvisPath::shader_build_path_str("imgui/imgui.slang"),
    },
});

pub struct GuiPass {
    pipeline: GfxGraphicsPipeline,
    pipeline_layout: Rc<GfxPipelineLayout>,
}
// new & init
impl GuiPass {
    pub fn new(render_descriptor_sets: &GlobalDescriptorSets, color_format: vk::Format) -> Self {
        let pipeline_layout = Rc::new(GfxPipelineLayout::new(
            &render_descriptor_sets.global_set_layouts(),
            &[vk::PushConstantRange {
                stage_flags: vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
                offset: 0,
                size: size_of::<gpu::imgui::PushConstant>() as u32,
            }],
            "uipass",
        ));

        let color_blend_attachments = vec![
            vk::PipelineColorBlendAttachmentState::default()
                .color_write_mask(
                    vk::ColorComponentFlags::R
                        | vk::ColorComponentFlags::G
                        | vk::ColorComponentFlags::B
                        | vk::ColorComponentFlags::A,
                )
                .blend_enable(true)
                .src_color_blend_factor(vk::BlendFactor::SRC_ALPHA)
                .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
                .color_blend_op(vk::BlendOp::ADD)
                .src_alpha_blend_factor(vk::BlendFactor::ONE)
                .dst_alpha_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
                .alpha_blend_op(vk::BlendOp::ADD),
        ];

        let mut create_info = GfxGraphicsPipelineCreateInfo::default();
        create_info
            .shader_stages(ShaderStage::iter().map(|stage| stage.value().clone()).collect_vec())
            .vertex_attribute(ImGuiVertexLayoutAoS::vertex_input_attributes())
            .vertex_binding(ImGuiVertexLayoutAoS::vertex_input_bindings())
            .cull_mode(vk::CullModeFlags::NONE, vk::FrontFace::CLOCKWISE)
            .color_blend(color_blend_attachments, [0.0; 4])
            .depth_test(Some(vk::CompareOp::ALWAYS), false, false)
            // TODO 这里不应该由 depth
            .attach_info(vec![color_format], None, None);

        let pipeline = GfxGraphicsPipeline::new(&create_info, pipeline_layout.clone(), "uipass");

        Self {
            pipeline,
            pipeline_layout,
        }
    }
}
// draw
impl GuiPass {
    pub fn draw(
        &self,
        frame_label: FrameLabel,
        global_descriptor_sets: &GlobalDescriptorSets,
        bindless_manager: &BindlessManager,
        canvas_color_view: vk::ImageView,
        canvas_extent: vk::Extent2D,
        cmd: &GfxCommandBuffer,
        gui_mesh: &GuiMesh,
        draw_data: &imgui::DrawData,
        tex_map: &HashMap<TextureId, GfxImageViewHandle>,
    ) {
        // 使用 LOAD 保留 resolve pass 绘制的内容
        let color_attach_info = vk::RenderingAttachmentInfo::default()
            .image_view(canvas_color_view)
            .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            .load_op(vk::AttachmentLoadOp::LOAD)
            .store_op(vk::AttachmentStoreOp::STORE);

        let render_info = vk::RenderingInfo::default()
            .layer_count(1)
            .render_area(canvas_extent.into())
            .color_attachments(std::slice::from_ref(&color_attach_info));

        let viewport = vk::Viewport {
            width: draw_data.framebuffer_scale[0] * draw_data.display_size[0],
            height: draw_data.framebuffer_scale[1] * draw_data.display_size[1],
            min_depth: 0.0,
            ..Default::default()
        };

        cmd.cmd_begin_rendering(&render_info);
        cmd.cmd_bind_pipeline(vk::PipelineBindPoint::GRAPHICS, self.pipeline.handle());
        cmd.cmd_set_viewport(0, std::slice::from_ref(&viewport));

        let mut push_constant = gpu::imgui::PushConstant {
            ortho: glam::Mat4::orthographic_rh(
                0.0,
                draw_data.display_size[0],
                0.0,
                draw_data.display_size[1],
                -1.0,
                1.0,
            )
            .into(),
            texture: SrvHandle {
                index: gpu::INVALID_TEX_ID,
            },
            texture_sampler_type: gpu::ESamplerType_LinearRepeat,
            _padding_0: Default::default(),
            _padding_1: Default::default(),
        };

        cmd.bind_descriptor_sets(
            vk::PipelineBindPoint::GRAPHICS,
            self.pipeline_layout.handle(),
            0,
            &global_descriptor_sets.global_sets(frame_label),
            None,
        );

        cmd.cmd_push_constants(
            self.pipeline_layout.handle(),
            vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
            0,
            BytesConvert::bytes_of(&push_constant),
        );

        cmd.cmd_bind_index_buffer(&gui_mesh.index_buffer, 0);
        cmd.cmd_bind_vertex_buffers(0, &[gui_mesh.vertex_buffer.vk_buffer()], &[0]);

        let mut index_offset = 0;
        let mut vertex_offset = 0;
        // 缓存之前已经加载过的 texture
        let mut last_texture_id: Option<imgui::TextureId> = None;
        let clip_offset = draw_data.display_pos;
        let clip_scale = draw_data.framebuffer_scale;

        // 简而言之：对于每个 command，设置正确的 vertex, index, texture, scissor 即可
        for draw_list in draw_data.draw_lists() {
            for command in draw_list.commands() {
                match command {
                    imgui::DrawCmd::Elements {
                        count,
                        cmd_params:
                            imgui::DrawCmdParams {
                                clip_rect,
                                texture_id, // 当前绘制命令用到的 texture，这个 id 是 app 决定的
                                vtx_offset,
                                idx_offset,
                            },
                    } => {
                        let clip_x = (clip_rect[0] - clip_offset[0]) * clip_scale[0];
                        let clip_y = (clip_rect[1] - clip_offset[1]) * clip_scale[1];
                        let clip_w = (clip_rect[2] - clip_offset[0]) * clip_scale[0] - clip_x;
                        let clip_h = (clip_rect[3] - clip_offset[1]) * clip_scale[1] - clip_y;

                        let scissors = [vk::Rect2D {
                            offset: vk::Offset2D {
                                x: (clip_x as i32).max(0),
                                y: (clip_y as i32).max(0),
                            },
                            extent: vk::Extent2D {
                                width: clip_w as _,
                                height: clip_h as _,
                            },
                        }];
                        cmd.cmd_set_scissor(0, &scissors);

                        // 加载 texture，如果和上一个 command 使用的 texture
                        // 不是同一个，则需要重新加载
                        if Some(texture_id) != last_texture_id {
                            let texture_image_view_handle = tex_map.get(&texture_id).unwrap();
                            let srv_bindless_handle =
                                bindless_manager.get_shader_srv_handle(*texture_image_view_handle);

                            push_constant.texture = srv_bindless_handle.0;

                            cmd.cmd_push_constants(
                                self.pipeline_layout.handle(),
                                vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
                                0,
                                BytesConvert::bytes_of(&push_constant),
                            );
                            last_texture_id = Some(texture_id);
                        }

                        cmd.draw_indexed(
                            count as u32,
                            index_offset + idx_offset as u32,
                            1,
                            0,
                            vertex_offset + vtx_offset as i32,
                        );
                    }
                    imgui::DrawCmd::ResetRenderState => {
                        log::warn!("imgui reset render state");
                    }
                    imgui::DrawCmd::RawCallback { .. } => {
                        log::warn!("imgui raw callback");
                    }
                }
            }

            index_offset += draw_list.idx_buffer().len() as u32;
            vertex_offset += draw_list.vtx_buffer().len() as i32;
        }
        cmd.end_rendering();
    }
}

