use crate::bindless_manager::BindlessManager;
use crate::frame_counter::FrameCounter;
use crate::gfx_resource_manager::GfxResourceManager;
use crate::gpu_scene::helper::ImageLoader;
use crate::handles::{GfxImageHandle, GfxImageViewHandle};
use crate::pipeline_settings::FrameLabel;
use crate::render_data::RenderData;
use ash::vk;
use itertools::Itertools;
use std::path::PathBuf;
use truvis_gfx::basic::bytes::BytesConvert;
use truvis_gfx::{
    commands::{
        barrier::{GfxBarrierMask, GfxBufferBarrier},
        command_buffer::GfxCommandBuffer,
    },
    raytracing::acceleration::GfxAcceleration,
    resources::special_buffers::structured_buffer::GfxStructuredBuffer,
};
use truvis_path::TruvisPath;
use truvis_shader_binding::gpu;

/// 构建 Gpu Scene 所需的所有 buffer
struct GpuSceneBuffers {
    scene_buffer: GfxStructuredBuffer<gpu::GPUScene>,
    light_buffer: GfxStructuredBuffer<gpu::PointLight>,
    light_stage_buffer: GfxStructuredBuffer<gpu::PointLight>,
    material_buffer: GfxStructuredBuffer<gpu::PBRMaterial>,
    material_stage_buffer: GfxStructuredBuffer<gpu::PBRMaterial>,
    geometry_buffer: GfxStructuredBuffer<gpu::Geometry>,
    geometry_stage_buffer: GfxStructuredBuffer<gpu::Geometry>,
    instance_buffer: GfxStructuredBuffer<gpu::Instance>,
    instance_stage_buffer: GfxStructuredBuffer<gpu::Instance>,
    material_indirect_buffer: GfxStructuredBuffer<u32>,
    material_indirect_stage_buffer: GfxStructuredBuffer<u32>,
    geometry_indirect_buffer: GfxStructuredBuffer<u32>,
    geometry_indirect_stage_buffer: GfxStructuredBuffer<u32>,

    // TODO 使用 frame id 来标记是否过期，scene manager 里面也需要有相应的标记
    tlas: Option<GfxAcceleration>,
}
// init & destroy
impl GpuSceneBuffers {
    fn new(frame_label: FrameLabel) -> Self {
        let max_light_cnt = 512;
        let max_material_cnt = 1024;
        let max_geometry_cnt = 1024 * 8;
        let max_instance_cnt = 1024;

        GpuSceneBuffers {
            scene_buffer: GfxStructuredBuffer::new_ubo(1, format!("scene buffer-{}", frame_label)),
            light_buffer: GfxStructuredBuffer::new_ssbo(max_light_cnt, format!("light buffer-{}", frame_label)),
            light_stage_buffer: GfxStructuredBuffer::new_stage_buffer(
                max_light_cnt,
                format!("light stage buffer-{}", frame_label),
            ),
            material_buffer: GfxStructuredBuffer::new_ssbo(
                max_material_cnt,
                format!("material buffer-{}", frame_label),
            ),
            material_stage_buffer: GfxStructuredBuffer::new_stage_buffer(
                max_material_cnt,
                format!("material stage buffer-{}", frame_label),
            ),
            geometry_buffer: GfxStructuredBuffer::new_ssbo(
                max_geometry_cnt,
                format!("geometry buffer-{}", frame_label),
            ),
            geometry_stage_buffer: GfxStructuredBuffer::new_stage_buffer(
                max_geometry_cnt,
                format!("geometry stage buffer-{}", frame_label),
            ),
            instance_buffer: GfxStructuredBuffer::new_ssbo(
                max_instance_cnt,
                format!("instance buffer-{}", frame_label),
            ),
            instance_stage_buffer: GfxStructuredBuffer::new_stage_buffer(
                max_instance_cnt,
                format!("instance stage buffer-{}", frame_label),
            ),
            material_indirect_buffer: GfxStructuredBuffer::new_ssbo(
                max_instance_cnt * 8,
                format!("instance material buffer-{}", frame_label),
            ),
            material_indirect_stage_buffer: GfxStructuredBuffer::new_stage_buffer(
                max_instance_cnt * 8,
                format!("instance material stage buffer-{}", frame_label),
            ),
            geometry_indirect_buffer: GfxStructuredBuffer::new_ssbo(
                max_instance_cnt * 8,
                format!("instance geometry buffer-{}", frame_label),
            ),
            geometry_indirect_stage_buffer: GfxStructuredBuffer::new_stage_buffer(
                max_instance_cnt * 8,
                format!("instance geometry stage buffer-{}", frame_label),
            ),
            tlas: None,
        }
    }
}

/// 用于构建传输到 GPU 的场景数据
pub struct GpuScene {
    gpu_scene_buffers: [GpuSceneBuffers; FrameCounter::fif_count()],

    // TODO sky texture handle 不应该放在 GPU scene 里面
    sky_texture: (GfxImageHandle, GfxImageViewHandle),
    // TODO uv checker texture handle 不应该放在 GPU scene 里面
    uv_checker_texture: (GfxImageHandle, GfxImageViewHandle),
}
// getter
impl GpuScene {
    #[inline]
    pub fn tlas(&self, frame_label: FrameLabel) -> Option<&GfxAcceleration> {
        self.gpu_scene_buffers[*frame_label].tlas.as_ref()
    }

    #[inline]
    pub fn scene_buffer(&self, frame_label: FrameLabel) -> &GfxStructuredBuffer<gpu::GPUScene> {
        &self.gpu_scene_buffers[*frame_label].scene_buffer
    }
}
// new & init
impl GpuScene {
    pub fn new(gfx_resource_manager: &mut GfxResourceManager, bindless_manager: &mut BindlessManager) -> Self {
        let sky_path = TruvisPath::resources_path_str("sky.jpg");
        let uv_checker_path = TruvisPath::resources_path_str("uv_checker.png");

        let sky_image = ImageLoader::load_image(&PathBuf::from(&sky_path));
        let uv_checker_image = ImageLoader::load_image(&PathBuf::from(&uv_checker_path));

        let sky_image_format = sky_image.format();
        let uv_checker_image_format = uv_checker_image.format();

        let sky_image_handle = gfx_resource_manager.register_image(sky_image);
        let sky_view_handle = gfx_resource_manager.get_or_create_image_view(
            sky_image_handle,
            truvis_gfx::resources::image_view::GfxImageViewDesc::new_2d(sky_image_format, vk::ImageAspectFlags::COLOR),
            &sky_path,
        );

        let uv_checker_image_handle = gfx_resource_manager.register_image(uv_checker_image);
        let uv_checker_view_handle = gfx_resource_manager.get_or_create_image_view(
            uv_checker_image_handle,
            truvis_gfx::resources::image_view::GfxImageViewDesc::new_2d(
                uv_checker_image_format,
                vk::ImageAspectFlags::COLOR,
            ),
            &uv_checker_path,
        );

        bindless_manager.register_srv(sky_view_handle);
        bindless_manager.register_srv(uv_checker_view_handle);

        Self {
            gpu_scene_buffers: FrameCounter::frame_labes().map(GpuSceneBuffers::new),

            sky_texture: (sky_image_handle, sky_view_handle),
            uv_checker_texture: (uv_checker_image_handle, uv_checker_view_handle),
        }
    }
}
impl Drop for GpuScene {
    fn drop(&mut self) {}
}
// destroy
impl GpuScene {
    pub fn destroy(self) {}
    pub fn destroy_mut(&mut self) {}
}
// tools
impl GpuScene {
    /// # Phase: Before Render (基于 SceneData2)
    ///
    /// 将已经准备好的 GPU 格式的场景数据写入 Device Buffer 中。
    /// 此方法不依赖 SceneManager，仅使用 SceneData2 中的数据。
    ///
    /// # 参数
    /// - `cmd`: 用于提交 GPU 命令的命令缓冲区
    /// - `barrier_mask`: 用于同步的屏障掩码
    /// - `frame_counter`: 帧计数器，用于获取当前帧的 buffer
    /// - `scene_data`: 包含完整场景信息的 SceneData2
    /// - `bindless_manager`: 用于获取 sky/uv_checker 等内置纹理的 bindless handle
    pub fn upload_render_data(
        &mut self,
        cmd: &GfxCommandBuffer,
        barrier_mask: GfxBarrierMask,
        frame_counter: &FrameCounter,
        render_data: &RenderData<'_>,
        bindless_manager: &BindlessManager,
    ) {
        let _span = tracy_client::span!("GpuScene::prepare_render_data2");

        self.upload_mesh_buffer(cmd, barrier_mask, render_data, frame_counter);
        self.upload_instance_buffer(cmd, barrier_mask, render_data, frame_counter);
        self.upload_material_buffer(cmd, barrier_mask, render_data, frame_counter);
        self.upload_light_buffer(cmd, barrier_mask, render_data, frame_counter);

        // 需要确保 instance 先于 tlas 构建
        self.build_tlas(render_data, frame_counter);

        self.upload_scene_buffer(cmd, frame_counter, barrier_mask, render_data, bindless_manager);
    }

    // TODO 改成：返回 Raster 模式的 RenderData
    /// 绘制场景中的所有实例（基于 SceneData2）
    ///
    /// # 参数
    /// - `cmd`: 命令缓冲区
    /// - `scene_data`: 场景数据
    /// - `before_draw`: 每次绘制前的回调函数 (instance_idx, submesh_idx)
    pub fn draw(&self, cmd: &GfxCommandBuffer, scene_data: &RenderData<'_>, mut before_draw: impl FnMut(u32, u32)) {
        let _span = tracy_client::span!("GpuScene::draw2");
        for (instance_idx, instance) in scene_data.all_instances.iter().enumerate() {
            let mesh = &scene_data.all_meshes[instance.mesh_index];
            for (submesh_idx, geometry) in mesh.geometries.iter().enumerate() {
                geometry.cmd_bind_index_buffer(cmd);
                geometry.cmd_bind_vertex_buffers(cmd);

                before_draw(instance_idx as u32, submesh_idx as u32);
                cmd.draw_indexed(geometry.index_cnt(), 0, 1, 0, 0);
            }
        }
    }
}

// 基于 SceneData2 的新方法
impl GpuScene {
    /// 将整个场景的数据上传到 scene buffer 中去（基于 SceneData2）
    fn upload_scene_buffer(
        &mut self,
        cmd: &GfxCommandBuffer,
        frame_counter: &FrameCounter,
        barrier_mask: GfxBarrierMask,
        scene_data: &RenderData<'_>,
        bindless_manager: &BindlessManager,
    ) {
        let crt_gpu_buffers = &self.gpu_scene_buffers[*frame_counter.frame_label()];
        let gpu_scene_data = gpu::GPUScene {
            all_instances: crt_gpu_buffers.instance_buffer.device_address(),
            all_mats: crt_gpu_buffers.material_buffer.device_address(),
            all_geometries: crt_gpu_buffers.geometry_buffer.device_address(),
            instance_material_map: crt_gpu_buffers.material_indirect_buffer.device_address(),
            instance_geometry_map: crt_gpu_buffers.geometry_indirect_buffer.device_address(),
            point_lights: crt_gpu_buffers.light_buffer.device_address(),
            spot_lights: 0, // TODO 暂时无用
            point_light_count: scene_data.all_point_lights.len() as u32,
            spot_light_count: 0, // TODO 暂时无用

            sky: bindless_manager.get_shader_srv_handle(self.sky_texture.1).0,
            sky_sampler_type: gpu::ESamplerType_LinearClamp,
            uv_checker: bindless_manager.get_shader_srv_handle(self.uv_checker_texture.1).0,
            uv_checker_sampler_type: gpu::ESamplerType_LinearClamp,
        };

        cmd.cmd_update_buffer(crt_gpu_buffers.scene_buffer.vk_buffer(), 0, BytesConvert::bytes_of(&gpu_scene_data));
        cmd.buffer_memory_barrier(
            vk::DependencyFlags::empty(),
            &[GfxBufferBarrier::default().mask(barrier_mask).buffer(
                crt_gpu_buffers.scene_buffer.vk_buffer(),
                0,
                vk::WHOLE_SIZE,
            )],
        );
    }

    /// 将 instance 数据上传到 GPU（基于 SceneData2）
    fn upload_instance_buffer(
        &mut self,
        cmd: &GfxCommandBuffer,
        barrier_mask: GfxBarrierMask,
        scene_data: &RenderData<'_>,
        frame_counter: &FrameCounter,
    ) {
        let _span = tracy_client::span!("upload_instance_buffer2");
        let crt_gpu_buffers = &mut self.gpu_scene_buffers[*frame_counter.frame_label()];

        let crt_instance_stage_buffer = &mut crt_gpu_buffers.instance_stage_buffer;
        let crt_geometry_indirect_stage_buffer = &mut crt_gpu_buffers.geometry_indirect_stage_buffer;
        let crt_material_indirect_stage_buffer = &mut crt_gpu_buffers.material_indirect_stage_buffer;

        let instance_buffer_slices = crt_instance_stage_buffer.mapped_slice();
        let material_indirect_buffer_slices = crt_material_indirect_stage_buffer.mapped_slice();
        let geometry_indirect_buffer_slices = crt_geometry_indirect_stage_buffer.mapped_slice();

        if instance_buffer_slices.len() < scene_data.all_instances.len() {
            panic!("instance cnt can not be larger than buffer");
        }

        let mut crt_geometry_indirect_idx = 0;
        let mut crt_material_indirect_idx = 0;
        for (instance_idx, instance) in scene_data.all_instances.iter().enumerate() {
            let submesh_cnt = instance.material_indices.len();
            if geometry_indirect_buffer_slices.len() < crt_geometry_indirect_idx + submesh_cnt {
                panic!("instance geometry cnt can not be larger than buffer");
            }
            if material_indirect_buffer_slices.len() < crt_material_indirect_idx + submesh_cnt {
                panic!("instance material cnt can not be larger than buffer");
            }

            instance_buffer_slices[instance_idx] = gpu::Instance {
                geometry_indirect_idx: crt_geometry_indirect_idx as u32,
                geometry_count: submesh_cnt as u32,
                material_indirect_idx: crt_material_indirect_idx as u32,
                material_count: submesh_cnt as u32,
                model: instance.transform.into(),
                inv_model: instance.transform.inverse().into(),
            };

            // 将 geometry 索引写入间接索引 buffer
            let mesh_startup_index = scene_data.mesh_geometry_start_indices[instance.mesh_index];
            for submesh_idx in 0..submesh_cnt {
                let geometry_idx = mesh_startup_index + submesh_idx;
                geometry_indirect_buffer_slices[crt_geometry_indirect_idx + submesh_idx] = geometry_idx as u32;
            }
            crt_geometry_indirect_idx += submesh_cnt;

            // 将 material 索引写入间接索引 buffer
            for material_index in instance.material_indices.iter() {
                material_indirect_buffer_slices[crt_material_indirect_idx] = *material_index as u32;
                crt_material_indirect_idx += 1;
            }
        }

        helper::flush_copy_and_barrier(
            cmd,
            crt_instance_stage_buffer,
            &mut crt_gpu_buffers.instance_buffer,
            barrier_mask,
        );
        helper::flush_copy_and_barrier(
            cmd,
            crt_geometry_indirect_stage_buffer,
            &mut crt_gpu_buffers.geometry_indirect_buffer,
            barrier_mask,
        );
        helper::flush_copy_and_barrier(
            cmd,
            crt_material_indirect_stage_buffer,
            &mut crt_gpu_buffers.material_indirect_buffer,
            barrier_mask,
        );
    }

    /// 将 material 数据上传到 GPU（基于 SceneData2）
    fn upload_material_buffer(
        &mut self,
        cmd: &GfxCommandBuffer,
        barrier_mask: GfxBarrierMask,
        scene_data: &RenderData<'_>,
        frame_counter: &FrameCounter,
    ) {
        let _span = tracy_client::span!("upload_material_buffer2");
        let crt_gpu_buffers = &mut self.gpu_scene_buffers[*frame_counter.frame_label()];
        let crt_material_stage_buffer = &mut crt_gpu_buffers.material_stage_buffer;
        let material_buffer_slices = crt_material_stage_buffer.mapped_slice();
        if material_buffer_slices.len() < scene_data.all_materials.len() {
            panic!("material cnt can not be larger than buffer");
        }

        for (mat_idx, mat) in scene_data.all_materials.iter().enumerate() {
            material_buffer_slices[mat_idx] = gpu::PBRMaterial {
                base_color: mat.base_color.truncate().into(),
                emissive: mat.emissive.truncate().into(),
                metallic: mat.metallic,
                roughness: mat.roughness,
                diffuse_map: mat.diffuse_bindless_handle.0,
                diffuse_map_sampler_type: gpu::ESamplerType_LinearRepeat,
                normal_map: mat.normal_bindless_handle.0,
                normal_map_sampler_type: gpu::ESamplerType_LinearRepeat,
                opaque: mat.opaque,
                _padding_1: Default::default(),
                _padding_2: Default::default(),
                _padding_3: Default::default(),
            };
        }

        helper::flush_copy_and_barrier(
            cmd,
            crt_material_stage_buffer,
            &mut crt_gpu_buffers.material_buffer,
            barrier_mask,
        );
    }

    /// 将 light 数据上传到 GPU（基于 SceneData2）
    fn upload_light_buffer(
        &mut self,
        cmd: &GfxCommandBuffer,
        barrier_mask: GfxBarrierMask,
        scene_data: &RenderData<'_>,
        frame_counter: &FrameCounter,
    ) {
        let _span = tracy_client::span!("upload_light_buffer2");
        let crt_gpu_buffers = &mut self.gpu_scene_buffers[*frame_counter.frame_label()];
        let crt_light_stage_buffer = &mut crt_gpu_buffers.light_stage_buffer;
        let light_buffer_slices = crt_light_stage_buffer.mapped_slice();
        if light_buffer_slices.len() < scene_data.all_point_lights.len() {
            panic!("light cnt can not be larger than buffer");
        }

        for (light_idx, point_light) in scene_data.all_point_lights.iter().enumerate() {
            light_buffer_slices[light_idx] = gpu::PointLight {
                pos: point_light.pos,
                color: point_light.color,

                _color_padding: Default::default(),
                _pos_padding: Default::default(),
            };
        }

        helper::flush_copy_and_barrier(cmd, crt_light_stage_buffer, &mut crt_gpu_buffers.light_buffer, barrier_mask);
    }

    /// 将 mesh 数据以 geometry 的形式上传到 GPU（基于 SceneData2）
    fn upload_mesh_buffer(
        &mut self,
        cmd: &GfxCommandBuffer,
        barrier_mask: GfxBarrierMask,
        scene_data: &RenderData<'_>,
        frame_counter: &FrameCounter,
    ) {
        let _span = tracy_client::span!("upload_mesh_buffer2");
        let crt_gpu_buffers = &mut self.gpu_scene_buffers[*frame_counter.frame_label()];
        let crt_geometry_stage_buffer = &mut crt_gpu_buffers.geometry_stage_buffer;
        let geometry_buffer_slices = crt_geometry_stage_buffer.mapped_slice();

        let mut crt_geometry_idx = 0;
        for mesh in scene_data.all_meshes.iter() {
            if geometry_buffer_slices.len() < crt_geometry_idx + mesh.geometries.len() {
                panic!("geometry cnt can not be larger than buffer");
            }
            for (submesh_idx, geometry) in mesh.geometries.iter().enumerate() {
                geometry_buffer_slices[crt_geometry_idx + submesh_idx] = gpu::Geometry {
                    position_buffer: geometry.vertex_buffer.pos_address(),
                    normal_buffer: geometry.vertex_buffer.normal_address(),
                    tangent_buffer: geometry.vertex_buffer.tangent_address(),
                    uv_buffer: geometry.vertex_buffer.uv_address(),
                    index_buffer: geometry.index_buffer.device_address(),
                };
            }
            crt_geometry_idx += mesh.geometries.len();
        }

        helper::flush_copy_and_barrier(
            cmd,
            crt_geometry_stage_buffer,
            &mut crt_gpu_buffers.geometry_buffer,
            barrier_mask,
        );
    }

    /// 根据 SceneData2 的 instance 信息获得加速结构的 instance 信息
    fn get_as_instance_info(
        &self,
        instance: &crate::render_data::InstanceRenderData,
        custom_idx: u32,
        scene_data: &RenderData<'_>,
    ) -> vk::AccelerationStructureInstanceKHR {
        let mesh = &scene_data.all_meshes[instance.mesh_index];
        vk::AccelerationStructureInstanceKHR {
            // 3x4 row-major matrix
            transform: helper::get_rt_matrix(&instance.transform),
            instance_custom_index_and_mask: vk::Packed24_8::new(custom_idx, 0xFF),
            instance_shader_binding_table_record_offset_and_flags: vk::Packed24_8::new(
                0, // TODO 暂时使用同一个 hit group
                vk::GeometryInstanceFlagsKHR::TRIANGLE_FACING_CULL_DISABLE.as_raw() as u8,
            ),
            acceleration_structure_reference: vk::AccelerationStructureReferenceKHR {
                device_handle: mesh.blas_device_address.expect("BLAS not built for mesh"),
            },
        }
    }

    /// 构建 TLAS（基于 SceneData2）
    fn build_tlas(&mut self, scene_data: &RenderData<'_>, frame_counter: &FrameCounter) {
        let _span = tracy_client::span!("build_tlas2");
        if scene_data.all_instances.is_empty() {
            // 没有实例数据，直接返回
            return;
        }

        if self.gpu_scene_buffers[*frame_counter.frame_label()].tlas.is_some() {
            // 已经构建过 tlas，直接返回
            return;
        }

        let instance_infos = scene_data
            .all_instances
            .iter()
            .enumerate()
            // BUG custom idx 的有效位数只有 24 位，如果场景内 instance 过多，可能会溢出
            .map(|(idx, ins)| self.get_as_instance_info(ins, idx as u32, scene_data))
            .collect_vec();
        let tlas = GfxAcceleration::build_tlas_sync(
            &instance_infos,
            vk::BuildAccelerationStructureFlagsKHR::empty(),
            format!("scene2-{}-{}", frame_counter.frame_label(), frame_counter.frame_id()),
        );

        self.gpu_scene_buffers[*frame_counter.frame_label()].tlas = Some(tlas);
    }
}

mod helper {
    use ash::vk;
    use truvis_gfx::resources::image::GfxImage;
    use truvis_gfx::{
        commands::{
            barrier::{GfxBarrierMask, GfxBufferBarrier},
            command_buffer::GfxCommandBuffer,
        },
        resources::buffer::GfxBuffer,
    };

    /// 三个操作：
    /// 1. 将 stage buffer 的数据 *全部* flush 到 buffer 中
    /// 2. 从 stage buffer 中将 *所有* 数据复制到目标 buffer 中
    /// 3. 添加 barrier，确保后续访问时 copy 已经完成且数据可用
    pub fn flush_copy_and_barrier(
        cmd: &GfxCommandBuffer,
        stage_buffer: &mut GfxBuffer,
        dst: &mut GfxBuffer,
        barrier_mask: GfxBarrierMask,
    ) {
        let buffer_size = stage_buffer.size();
        {
            stage_buffer.flush(0, buffer_size);
        }
        cmd.cmd_copy_buffer(
            stage_buffer,
            dst,
            &[vk::BufferCopy {
                size: buffer_size,
                ..Default::default()
            }],
        );
        cmd.buffer_memory_barrier(
            vk::DependencyFlags::empty(),
            &[GfxBufferBarrier::default().mask(barrier_mask).buffer(dst.vk_buffer(), 0, vk::WHOLE_SIZE)],
        );
    }

    pub fn get_rt_matrix(trans: &glam::Mat4) -> vk::TransformMatrixKHR {
        let c1 = &trans.x_axis;
        let c2 = &trans.y_axis;
        let c3 = &trans.z_axis;
        let c4 = &trans.w_axis;

        // 3x4 matrix, row-major order
        vk::TransformMatrixKHR {
            matrix: [
                c1.x, c2.x, c3.x, c4.x, // row 1
                c1.y, c2.y, c3.y, c4.y, // row 2
                c1.z, c2.z, c3.z, c4.z, // row 3
            ],
        }
    }

    // TODO 临时的图片加载器，后续需要整合到 TextureManager 中
    pub struct ImageLoader {}
    impl ImageLoader {
        pub fn load_image(tex_path: &std::path::Path) -> GfxImage {
            let img = image::ImageReader::open(tex_path).unwrap().decode().unwrap().to_rgba8();
            let width = img.width();
            let height = img.height();
            let data = img.as_raw();
            let name = tex_path.to_str().unwrap();

            GfxImage::from_rgba8(width, height, data, name)
        }
    }
}
