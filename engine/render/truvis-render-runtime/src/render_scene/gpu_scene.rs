use super::buffers::GpuSceneBuffers;
use super::raster_draw_cache::{RasterDrawItem, draw_raster_cache, update_raster_draw_cache};
use crate::render_scene::render_data::{InstanceRenderData, RenderData};
use crate::scene_sync::emissive_light_table::EmissiveLightBinding;
use crate::scene_sync::environment_binding::EnvironmentBinding;
use ash::vk;
use itertools::Itertools;
use truvis_gfx::basic::bytes::BytesConvert;
use truvis_gfx::commands::barrier::{GfxBarrierMask, GfxBufferBarrier};
use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_gfx::gfx::{GfxDeviceCtx, GfxImmediateCtx, GfxResourceCtx};
use truvis_gfx::raytracing::acceleration::GfxAcceleration;
use truvis_gfx::resources::buffer::GfxBuffer;
use truvis_gfx::resources::lifecycle::DestroyReason;
use truvis_gfx::resources::special_buffers::structured_buffer::GfxStructuredBuffer;
use truvis_render_foundation::frame_counter::FrameCounter;
use truvis_render_foundation::frame_counter::FrameLabel;
use truvis_render_foundation::render_scene_view::{RenderSceneAccumSignature, RenderSceneView};
use truvis_shader_binding::gpu;

/// runtime 私有的 GPU scene 翻译层。
///
/// 它把 `InstanceBridge` 产出的 `RenderData` 转换成 shader 可读的 GPU buffer、TLAS 和
/// 光栅化 draw cache，并通过 `RenderSceneView` 向 render pass 暴露只读能力。
/// `GpuScene` 不拥有 CPU scene；它只保存当前 FIF 可用的 GPU 表示。
pub struct GpuScene {
    /// 每个 FIF frame label 独占一套 scene buffer 与 TLAS，避免覆盖 GPU 仍在读取的数据。
    pub(super) gpu_scene_buffers: [GpuSceneBuffers; FrameCounter::fif_count()],
    /// prepare 阶段从 `RenderData` 展开的光栅化 draw cache，render pass 只通过 view 契约录制 draw。
    pub(super) raster_draws: [Vec<RasterDrawItem>; FrameCounter::fif_count()],
}

// 生命周期：创建和销毁 `GpuScene` 拥有的长期 GPU 资源。
impl GpuScene {
    /// 创建每个 FIF frame label 的 GPU scene buffer 集。
    ///
    /// `GpuScene` 只创建 render-side 表示，不读取 CPU scene；动态实例和 mesh 数据在
    /// prepare 阶段由 `upload_render_data` 写入。
    pub fn new(resource_ctx: GfxResourceCtx<'_>) -> Self {
        let _span = tracy_client::span!("GpuScene::new");

        let gpu_scene_buffers = {
            let _span = tracy_client::span!("GpuScene::new/per_frame_buffers");
            FrameCounter::frame_labes().map(|frame_label| GpuSceneBuffers::new(resource_ctx, frame_label))
        };

        Self {
            gpu_scene_buffers,
            raster_draws: FrameCounter::frame_labes().map(|_| Vec::new()),
        }
    }

    /// 销毁 GPU scene 拥有的 buffer 和 TLAS。
    ///
    /// 调用点位于 `RenderRuntime::destroy`，此时 device 已 idle，因此每个 FIF 的 TLAS
    /// 和 buffer 都可以按 shutdown reason 释放。
    pub fn destroy_mut(&mut self, resource_ctx: GfxResourceCtx<'_>, device_ctx: GfxDeviceCtx<'_>) {
        for buffers in &mut self.gpu_scene_buffers {
            buffers.destroy_mut(resource_ctx, device_ctx);
        }
    }
}

// 只读访问器：供 runtime 内部和 `RenderSceneView` 暴露当前 FIF 的 scene 表示。
impl GpuScene {
    /// 返回当前 frame label 的 TLAS owner。
    ///
    /// 没有 active instance 时返回 None，render pass 应据此跳过 ray tracing 路径或使用空场景策略。
    #[inline]
    pub fn tlas(&self, frame_label: FrameLabel) -> Option<&GfxAcceleration> {
        self.gpu_scene_buffers[*frame_label].tlas.as_ref()
    }

    /// 返回当前 frame label 的 scene root buffer。
    ///
    /// buffer 内保存 shader 查找整套 scene 数据所需的 device address、bindless handle 和计数。
    #[inline]
    pub fn scene_buffer(&self, frame_label: FrameLabel) -> &GfxStructuredBuffer<gpu::scene::GpuScene> {
        &self.gpu_scene_buffers[*frame_label].scene_buffer
    }
}

// Render pass 可见契约：隐藏 `GpuScene` owner，只暴露 scene root、TLAS 与 draw 录制能力。
impl RenderSceneView for GpuScene {
    /// 暴露 scene root buffer device address，供 shader 通过全局 descriptor 间接读取场景。
    fn scene_buffer_device_address(&self, frame_label: FrameLabel) -> vk::DeviceAddress {
        self.scene_buffer(frame_label).device_address()
    }

    /// 暴露当前 frame label 的 TLAS handle；空场景没有 TLAS。
    fn tlas_handle(&self, frame_label: FrameLabel) -> Option<vk::AccelerationStructureKHR> {
        self.tlas(frame_label).map(|tlas| tlas.handle())
    }

    fn accum_signature(&self, frame_label: FrameLabel) -> RenderSceneAccumSignature {
        self.gpu_scene_buffers[*frame_label].accum_signature
    }

    /// 遍历 prepare 阶段展开好的 raster draw cache 并录制 draw。
    ///
    /// `before_draw` 是 pass 注入点，用于按 instance slot/submesh index 更新 push constants
    /// 或其它 per-draw 状态，同时不暴露 `GpuScene` 的内部缓存结构。
    fn draw_raster(&self, frame_label: FrameLabel, cmd: &GfxCommandBuffer, before_draw: &mut dyn FnMut(u32, u32)) {
        let _span = tracy_client::span!("GpuScene::draw_raster");
        draw_raster_cache(&self.raster_draws[*frame_label], cmd, before_draw);
    }
}

// Prepare 上传：把 `RenderData` 写入当前 FIF 的 GPU buffer，并刷新本帧可见的 scene root。
impl GpuScene {
    /// # 阶段：Before Render
    ///
    /// 将 runtime bridge 已经整理好的 `RenderData` 写入当前 FIF 的 device buffer。
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
        environment_binding: EnvironmentBinding,
        emissive_light_binding: EmissiveLightBinding,
    ) {
        let _span = tracy_client::span!("GpuScene::prepare_render_data2");

        update_raster_draw_cache(&mut self.raster_draws[*frame_counter.frame_label()], render_data);
        self.upload_mesh_buffer(resource_ctx, cmd, barrier_mask, render_data, frame_counter);
        self.upload_instance_buffer(resource_ctx, cmd, barrier_mask, render_data, frame_counter);
        self.upload_light_buffer(resource_ctx, cmd, barrier_mask, render_data, frame_counter);

        // TLAS instance 描述使用稳定 instance slot 与 transform，因此必须在 instance buffer
        // 写入逻辑之后构建，保证 GPU scene buffer、TLAS custom index 和 raster draw cache 对齐。
        self.build_tlas(resource_ctx, device_ctx, immediate_ctx, render_data, frame_counter, tlas_revision);

        self.upload_scene_buffer(
            cmd,
            frame_counter,
            barrier_mask,
            render_data,
            material_buffer_device_address,
            environment_binding,
            emissive_light_binding,
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
        environment_binding: EnvironmentBinding,
        emissive_light_binding: EmissiveLightBinding,
    ) {
        let crt_gpu_buffers = &self.gpu_scene_buffers[*frame_counter.frame_label()];
        // scene root buffer 只存放“入口地址”和资源句柄，不复制大块 scene 数据。
        // 它最后写入，确保地址/count 与本帧刚上传的 buffer 和 TLAS revision 匹配。
        let gpu_scene_data = gpu::scene::GpuScene {
            all_instances: crt_gpu_buffers.instance_buffer.device_address(),
            all_mats: material_buffer_device_address,
            all_geometries: crt_gpu_buffers.geometry_buffer.device_address(),
            instance_material_map: crt_gpu_buffers.material_indirect_buffer.device_address(),
            instance_geometry_map: crt_gpu_buffers.geometry_indirect_buffer.device_address(),
            point_lights: crt_gpu_buffers.point_light_buffer.device_address(),
            spot_lights: crt_gpu_buffers.spot_light_buffer.device_address(),
            area_lights: crt_gpu_buffers.area_light_buffer.device_address(),
            emissive_triangle_lights: emissive_light_binding.triangle_lights_device_address,
            emissive_light_alias_table: emissive_light_binding.alias_table_device_address,
            instance_emissive_triangle_base_map: emissive_light_binding.base_map_device_address,
            emissive_light_count: emissive_light_binding.alias_count,
            emissive_light_enabled: emissive_light_binding.enabled,
            emissive_light_version: emissive_light_binding.version,
            emissive_light_record_count: emissive_light_binding.record_count,
            point_light_count: scene_data.all_point_lights.len() as u32,
            spot_light_count: scene_data.all_spot_lights.len() as u32,
            area_light_count: scene_data.all_area_lights.len() as u32,
            analytic_light_version: scene_data.analytic_light_version,

            sky: environment_binding.sky.srv_handle.0,
            sky_sampler_type: environment_binding.sky.sampler,
            sky_distribution: environment_binding.sky.distribution_device_address,
            sky_distribution_width: environment_binding.sky.distribution_width,
            sky_distribution_height: environment_binding.sky.distribution_height,
            sky_distribution_enabled: environment_binding.sky.distribution_enabled,
            sky_distribution_version: environment_binding.sky.distribution_version,
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

        // 累计签名必须在本帧 scene root buffer、TLAS、light 与 sky binding 都确定后写入。
        // 它只描述会让离线 reference 历史失效的语义版本，不暴露具体 GPU buffer 所有权。
        let accum_signature = RenderSceneAccumSignature {
            tlas_revision: crt_gpu_buffers.tlas_revision,
            emissive_light_version: emissive_light_binding.version,
            analytic_light_version: scene_data.analytic_light_version,
            sky_distribution_version: environment_binding.sky.distribution_version,
        };
        self.gpu_scene_buffers[*frame_counter.frame_label()].accum_signature = accum_signature;
    }

    /// 将 mesh 数据以 geometry 表的形式上传到 GPU。
    ///
    /// geometry 表只保存 device address；实际 vertex/index buffer 生命周期由 mesh manager 持有。
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
            // RenderData 已经按 mesh 去重；这里把每个 mesh 的 submesh 展开为全局 geometry table，
            // instance 只通过 geometry_indirect_buffer 指向其中一段连续范围。
            if geometry_buffer_slices.len() < crt_geometry_idx + mesh.geometries.len() {
                panic!("geometry cnt can not be larger than buffer");
            }
            for (submesh_idx, geometry) in mesh.geometries.iter().enumerate() {
                geometry_buffer_slices[crt_geometry_idx + submesh_idx] = gpu::geometry::Geometry {
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
            // instance buffer 使用全局稳定 slot 下标写入；indirect buffer 使用本帧紧凑下标写入。
            // 这种布局让 shader 可以先用 TLAS/raster 提供的 instance slot 找到 instance，
            // 再通过 instance 中的 indirect range 找到对应 submesh 列表。
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

            instance_buffer_slices[instance_slot] = gpu::scene::Instance {
                geometry_indirect_idx: crt_geometry_indirect_idx as u32,
                geometry_count: submesh_cnt as u32,
                material_indirect_idx: crt_material_indirect_idx as u32,
                material_count: submesh_cnt as u32,
                model: instance.transform.into(),
                inv_model: instance.transform.inverse().into(),
                prev_model: instance.previous_transform.into(),
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
        {
            let point_light_buffer_slices = crt_gpu_buffers.point_light_stage_buffer.mapped_slice();
            let spot_light_buffer_slices = crt_gpu_buffers.spot_light_stage_buffer.mapped_slice();
            let area_light_buffer_slices = crt_gpu_buffers.area_light_stage_buffer.mapped_slice();
            // 当前实现使用固定容量 analytic light buffer；超过容量说明 scene 规模已超出 runtime v1 约束。
            if point_light_buffer_slices.len() < scene_data.all_point_lights.len()
                || spot_light_buffer_slices.len() < scene_data.all_spot_lights.len()
                || area_light_buffer_slices.len() < scene_data.all_area_lights.len()
            {
                panic!("analytic light cnt can not be larger than buffer");
            }

            for (light_idx, point_light) in scene_data.all_point_lights.iter().enumerate() {
                point_light_buffer_slices[light_idx] = gpu::light::PointLight {
                    pos: point_light.pos,
                    color: point_light.color,

                    _color_padding: Default::default(),
                    _pos_padding: Default::default(),
                };
            }

            for (light_idx, spot_light) in scene_data.all_spot_lights.iter().enumerate() {
                spot_light_buffer_slices[light_idx] = gpu::light::SpotLight {
                    pos: spot_light.pos,
                    inner_angle: spot_light.inner_angle,
                    color: spot_light.color,
                    outer_angle: spot_light.outer_angle,
                    dir: spot_light.dir,
                    _dir_padding: Default::default(),
                };
            }

            for (light_idx, area_light) in scene_data.all_area_lights.iter().enumerate() {
                area_light_buffer_slices[light_idx] = gpu::light::AreaLight {
                    center: area_light.center,
                    half_u: area_light.half_u,
                    half_v: area_light.half_v,
                    radiance: area_light.radiance,

                    _center_padding: Default::default(),
                    _half_u_padding: Default::default(),
                    _half_v_padding: Default::default(),
                    _radiance_padding: Default::default(),
                };
            }
        }

        flush_copy_and_barrier(
            resource_ctx,
            cmd,
            &mut crt_gpu_buffers.point_light_stage_buffer,
            &mut crt_gpu_buffers.point_light_buffer,
            barrier_mask,
        );
        flush_copy_and_barrier(
            resource_ctx,
            cmd,
            &mut crt_gpu_buffers.spot_light_stage_buffer,
            &mut crt_gpu_buffers.spot_light_buffer,
            barrier_mask,
        );
        flush_copy_and_barrier(
            resource_ctx,
            cmd,
            &mut crt_gpu_buffers.area_light_stage_buffer,
            &mut crt_gpu_buffers.area_light_buffer,
            barrier_mask,
        );
    }
}

// TLAS 构建：按当前 FIF 的 scene revision 维护 ray tracing 可见的 acceleration structure。
impl GpuScene {
    /// 构建当前 FIF 的 TLAS。
    ///
    /// `tlas_revision` 由 mesh ready revision 与 instance bridge revision 组成；当 mesh BLAS ready、
    /// instance 增删、激活状态或 transform 改变时才重建，避免每帧无意义重建 TLAS。
    pub(super) fn build_tlas(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        scene_data: &RenderData<'_>,
        frame_counter: &FrameCounter,
        tlas_revision: u64,
    ) {
        let _span = tracy_client::span!("build_tlas2");
        let frame_index = *frame_counter.frame_label();
        if scene_data.all_instances.is_empty() {
            // 空场景不保留旧 TLAS。这样 render pass 通过 `tlas_handle == None`
            // 可以明确知道当前 frame label 没有可追踪实例。
            if let Some(tlas) = self.gpu_scene_buffers[frame_index].tlas.take() {
                tlas.destroy(resource_ctx, device_ctx, DestroyReason::ImmediateRelease);
            }
            self.gpu_scene_buffers[frame_index].tlas_revision = tlas_revision;
            return;
        }

        if self.gpu_scene_buffers[frame_index].tlas.is_some()
            && self.gpu_scene_buffers[frame_index].tlas_revision == tlas_revision
        {
            // 当前 FIF 的 TLAS 已经覆盖相同 scene revision，复用旧 acceleration structure。
            return;
        }

        // custom index 使用稳定 instance slot，ray tracing shader 可以和 raster path 共用
        // 同一套 GPU instance buffer 查找逻辑。
        let instance_infos = scene_data
            .all_instances
            .iter()
            .map(|ins| {
                ins.instance_slot.validate_tlas_custom_index();
                self.get_as_instance_info(ins, ins.instance_slot.as_u32(), scene_data)
            })
            .collect_vec();

        if let Some(tlas) = self.gpu_scene_buffers[frame_index].tlas.take() {
            // 这里释放的是当前 frame label 的旧 TLAS；begin_frame 的 FIF timeline wait
            // 保证同一 label 上一次提交已经结束。
            tlas.destroy(resource_ctx, device_ctx, DestroyReason::ImmediateRelease);
        }

        // 当前封装使用同步 build helper，适合 prepare 阶段的小规模 scene v1。
        // 如果后续改为异步 build，需要同步更新 scene root buffer 与 render graph 等待关系。
        let tlas = GfxAcceleration::build_tlas_sync(
            resource_ctx,
            device_ctx,
            immediate_ctx,
            &instance_infos,
            vk::BuildAccelerationStructureFlagsKHR::empty(),
            format!("scene2-{}-{}", frame_counter.frame_label(), frame_counter.frame_id()),
        );

        self.gpu_scene_buffers[frame_index].tlas = Some(tlas);
        self.gpu_scene_buffers[frame_index].tlas_revision = tlas_revision;
    }

    /// 根据 `RenderData` 的 instance 信息生成 TLAS instance 描述。
    ///
    /// `custom_idx` 使用稳定 instance slot，ray tracing shader 可以用它回查 GPU instance buffer。
    pub(super) fn get_as_instance_info(
        &self,
        instance: &InstanceRenderData,
        custom_idx: u32,
        scene_data: &RenderData<'_>,
    ) -> vk::AccelerationStructureInstanceKHR {
        let mesh = &scene_data.all_meshes[instance.mesh_index];
        // Vulkan TLAS instance transform 是 3x4 row-major；glam 的 Mat4 是列向量布局，
        // 需要在 `rt_transform_matrix` 中转置成 Vulkan 期望的行数据。
        vk::AccelerationStructureInstanceKHR {
            transform: rt_transform_matrix(&instance.transform),
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
}

/// 将 runtime 使用的 `glam::Mat4` 转换为 Vulkan TLAS instance transform。
///
/// Vulkan 结构只接受 3x4 row-major 矩阵，最后一行隐含为 `[0, 0, 0, 1]`。
fn rt_transform_matrix(trans: &glam::Mat4) -> vk::TransformMatrixKHR {
    let c1 = &trans.x_axis;
    let c2 = &trans.y_axis;
    let c3 = &trans.z_axis;
    let c4 = &trans.w_axis;

    vk::TransformMatrixKHR {
        matrix: [
            c1.x, c2.x, c3.x, c4.x, // 第 1 行
            c1.y, c2.y, c3.y, c4.y, // 第 2 行
            c1.z, c2.z, c3.z, c4.z, // 第 3 行
        ],
    }
}

/// 三个操作：
/// 1. 将 stage buffer 的数据 *全部* flush 到 buffer 中
/// 2. 从 stage buffer 中将 *所有* 数据复制到目标 buffer 中
/// 3. 添加 barrier，确保后续访问时 Copy 已经完成且数据可用
///
/// 当前 scene buffer 上传采用整 buffer copy，简化 dirty tracking；调用者负责传入后续 shader
/// 阶段需要的可见性 mask。
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
