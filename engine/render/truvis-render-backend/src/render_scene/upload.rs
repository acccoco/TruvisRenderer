use ash::vk;

use truvis_gfx::basic::bytes::BytesConvert;
use truvis_gfx::commands::barrier::{GfxBarrierMask, GfxBufferBarrier};
use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_gfx::gfx::{GfxDeviceCtx, GfxImmediateCtx, GfxResourceCtx};
use truvis_gfx::resources::buffer::GfxBuffer;
use truvis_gfx::resources::special_buffers::structured_buffer::GfxStructuredBuffer;
use truvis_render_interface::bindless_manager::BindlessManager;
use truvis_render_interface::frame_counter::FrameCounter;
use truvis_shader_binding::gpu;

use super::gpu_scene::GpuScene;
use super::raster_draw_cache::update_raster_draw_cache;
use super::render_data::RenderData;

impl GpuScene {
    /// # 阶段：Before Render
    ///
    /// 将 backend bridge 已经整理好的 `RenderData` 写入当前 FIF 的 device buffer。
    /// 此方法不依赖 `SceneManager`，这是 CPU scene 与 GPU scene 的边界。
    ///
    /// 上传顺序刻意保持为 draw cache、mesh/instance/light buffer、TLAS、scene root buffer：
    /// scene root buffer 最后写入，确保它记录的 device address 与本帧实际 buffer/TLAS 对齐。
    pub fn upload_render_data(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        cmd: &GfxCommandBuffer,
        barrier_mask: GfxBarrierMask,
        frame_counter: &FrameCounter,
        render_data: &RenderData<'_>,
        material_buffer_device_address: vk::DeviceAddress,
        tlas_revision: u64,
        bindless_manager: &BindlessManager,
    ) {
        let _span = tracy_client::span!("GpuScene::prepare_render_data2");

        update_raster_draw_cache(&mut self.raster_draws[*frame_counter.frame_label()], render_data);
        self.upload_mesh_buffer(resource_ctx, cmd, barrier_mask, render_data, frame_counter);
        self.upload_instance_buffer(resource_ctx, cmd, barrier_mask, render_data, frame_counter);
        self.upload_light_buffer(resource_ctx, cmd, barrier_mask, render_data, frame_counter);

        // 需要确保 instance 先于 tlas 构建
        self.build_tlas(resource_ctx, device_ctx, immediate_ctx, render_data, frame_counter, tlas_revision);

        self.upload_scene_buffer(
            cmd,
            frame_counter,
            barrier_mask,
            render_data,
            material_buffer_device_address,
            bindless_manager,
        );
    }

    /// 将 GPU scene root 数据上传到 scene buffer。
    ///
    /// 这个 buffer 只保存 device address、bindless handle 和计数，是 shader 访问整套场景数据的入口。
    fn upload_scene_buffer(
        &mut self,
        cmd: &GfxCommandBuffer,
        frame_counter: &FrameCounter,
        barrier_mask: GfxBarrierMask,
        scene_data: &RenderData<'_>,
        material_buffer_device_address: vk::DeviceAddress,
        bindless_manager: &BindlessManager,
    ) {
        let crt_gpu_buffers = &self.gpu_scene_buffers[*frame_counter.frame_label()];
        let gpu_scene_data = gpu::GPUScene {
            all_instances: crt_gpu_buffers.instance_buffer.device_address(),
            all_mats: material_buffer_device_address,
            all_geometries: crt_gpu_buffers.geometry_buffer.device_address(),
            instance_material_map: crt_gpu_buffers.material_indirect_buffer.device_address(),
            instance_geometry_map: crt_gpu_buffers.geometry_indirect_buffer.device_address(),
            point_lights: crt_gpu_buffers.light_buffer.device_address(),
            spot_lights: 0, // TODO 暂时无用
            point_light_count: scene_data.all_point_lights.len() as u32,
            spot_light_count: 0, // TODO 暂时无用

            sky: self.environment.sky_srv_handle(bindless_manager).0,
            sky_sampler_type: gpu::ESamplerType_LinearClamp,
            uv_checker: self.environment.uv_checker_srv_handle(bindless_manager).0,
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

    /// 将 instance 数据上传到 GPU。
    ///
    /// `instance_slot` 是全局稳定 slot；geometry/material indirect buffer 则是本帧紧凑列表，
    /// 用于把一个 instance 映射到它的 submesh geometry 与 material slot。
    fn upload_instance_buffer(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
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

        let mut crt_geometry_indirect_idx = 0;
        let mut crt_material_indirect_idx = 0;
        for instance in scene_data.all_instances.iter() {
            let instance_slot = instance.instance_slot.as_usize();
            if instance_buffer_slices.len() <= instance_slot {
                panic!("instance slot can not be larger than buffer");
            }

            let submesh_cnt = instance.material_slots.len();
            if geometry_indirect_buffer_slices.len() < crt_geometry_indirect_idx + submesh_cnt {
                panic!("instance geometry cnt can not be larger than buffer");
            }
            if material_indirect_buffer_slices.len() < crt_material_indirect_idx + submesh_cnt {
                panic!("instance material cnt can not be larger than buffer");
            }

            instance_buffer_slices[instance_slot] = gpu::Instance {
                geometry_indirect_idx: crt_geometry_indirect_idx as u32,
                geometry_count: submesh_cnt as u32,
                material_indirect_idx: crt_material_indirect_idx as u32,
                material_count: submesh_cnt as u32,
                model: instance.transform.into(),
                inv_model: instance.transform.inverse().into(),
            };

            // 将 geometry 索引写入间接索引 buffer。
            // mesh 在 RenderData 中去重，instance 只保存它引用的 submesh 范围。
            let mesh_startup_index = scene_data.mesh_geometry_start_indices[instance.mesh_index];
            for submesh_idx in 0..submesh_cnt {
                let geometry_idx = mesh_startup_index + submesh_idx;
                geometry_indirect_buffer_slices[crt_geometry_indirect_idx + submesh_idx] = geometry_idx as u32;
            }
            crt_geometry_indirect_idx += submesh_cnt;

            // 将稳定 material slot 写入间接索引 buffer。
            // material slot 来自 MaterialManager，可被 shader 直接索引材质 buffer。
            for material_slot in instance.material_slots.iter() {
                material_indirect_buffer_slices[crt_material_indirect_idx] = *material_slot;
                crt_material_indirect_idx += 1;
            }
        }

        flush_copy_and_barrier(
            resource_ctx,
            cmd,
            crt_instance_stage_buffer,
            &mut crt_gpu_buffers.instance_buffer,
            barrier_mask,
        );
        flush_copy_and_barrier(
            resource_ctx,
            cmd,
            crt_geometry_indirect_stage_buffer,
            &mut crt_gpu_buffers.geometry_indirect_buffer,
            barrier_mask,
        );
        flush_copy_and_barrier(
            resource_ctx,
            cmd,
            crt_material_indirect_stage_buffer,
            &mut crt_gpu_buffers.material_indirect_buffer,
            barrier_mask,
        );
    }

    /// 将 light 快照上传到当前 FIF 的 GPU buffer。
    fn upload_light_buffer(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
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

        flush_copy_and_barrier(
            resource_ctx,
            cmd,
            crt_light_stage_buffer,
            &mut crt_gpu_buffers.light_buffer,
            barrier_mask,
        );
    }

    /// 将 mesh 数据以 geometry 表的形式上传到 GPU。
    ///
    /// geometry 表只保存 device address；实际 vertex/index buffer 生命周期由 mesh uploader 持有。
    fn upload_mesh_buffer(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
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

        flush_copy_and_barrier(
            resource_ctx,
            cmd,
            crt_geometry_stage_buffer,
            &mut crt_gpu_buffers.geometry_buffer,
            barrier_mask,
        );
    }
}

/// 三个操作：
/// 1. 将 stage buffer 的数据 *全部* flush 到 buffer 中
/// 2. 从 stage buffer 中将 *所有* 数据复制到目标 buffer 中
/// 3. 添加 barrier，确保后续访问时 Copy 已经完成且数据可用
fn flush_copy_and_barrier(
    resource_ctx: GfxResourceCtx<'_>,
    cmd: &GfxCommandBuffer,
    stage_buffer: &mut GfxBuffer,
    dst: &mut GfxBuffer,
    barrier_mask: GfxBarrierMask,
) {
    let buffer_size = stage_buffer.size();
    stage_buffer.flush(resource_ctx, 0, buffer_size);
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

#[allow(dead_code)]
fn flush_structured_copy_and_barrier<T: Copy>(
    resource_ctx: GfxResourceCtx<'_>,
    cmd: &GfxCommandBuffer,
    stage_buffer: &mut GfxStructuredBuffer<T>,
    dst_buffer: &mut GfxStructuredBuffer<T>,
    barrier_mask: GfxBarrierMask,
) {
    flush_copy_and_barrier(resource_ctx, cmd, stage_buffer, dst_buffer, barrier_mask);
}
