use ash::vk;

use truvis_gfx::commands::barrier::{GfxBarrierMask, GfxBufferBarrier};
use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_gfx::gfx::GfxResourceCtx;
use truvis_gfx::resources::lifecycle::DestroyReason;
use truvis_gfx::resources::special_buffers::structured_buffer::GfxStructuredBuffer;
use truvis_render_foundation::frame_counter::{FrameCounter, FrameLabel};
use truvis_shader_binding::gpu;
use truvis_world::{SceneMaterialEmissiveView, SceneReadView};

use crate::render_world::geometry::RtTriangleMeta;
use crate::render_world::render_data::{InstanceRenderData, MeshRenderData, RenderData};

const INVALID_EMISSIVE_TRIANGLE_BASE: u32 = u32::MAX;

/// 写入 `scene::GpuScene` 的自发光 light table 绑定快照。
#[derive(Clone, Copy)]
pub(crate) struct EmissiveLightBinding {
    pub(crate) triangle_lights_device_address: vk::DeviceAddress,
    pub(crate) alias_table_device_address: vk::DeviceAddress,
    pub(crate) base_map_device_address: vk::DeviceAddress,
    pub(crate) alias_count: u32,
    pub(crate) enabled: u32,
    pub(crate) version: u32,
    /// triangle_lights 的实际 record 数量。alias_count 只描述可被 NEE 抽样的 entry 数；
    /// ReSTIR history 重建旧 emissive key 时需要用 record_count 做直接索引越界保护。
    pub(crate) record_count: u32,
}

/// 单个 FIF frame label 使用的一组自发光 light table buffer。
struct EmissiveLightFrameBuffers {
    triangle_lights: GfxStructuredBuffer<gpu::light::EmissiveTriangleLight>,
    triangle_lights_stage: GfxStructuredBuffer<gpu::light::EmissiveTriangleLight>,
    alias_table: GfxStructuredBuffer<gpu::light::EmissiveLightAliasEntry>,
    alias_table_stage: GfxStructuredBuffer<gpu::light::EmissiveLightAliasEntry>,
    base_map: GfxStructuredBuffer<u32>,
    base_map_stage: GfxStructuredBuffer<u32>,
    triangle_capacity: usize,
    alias_capacity: usize,
    base_map_capacity: usize,
    frame_label: FrameLabel,
}

impl EmissiveLightFrameBuffers {
    fn new(resource_ctx: GfxResourceCtx<'_>, frame_label: FrameLabel) -> Self {
        Self::with_capacity(resource_ctx, frame_label, 1, 1, 1)
    }

    fn with_capacity(
        resource_ctx: GfxResourceCtx<'_>,
        frame_label: FrameLabel,
        triangle_capacity: usize,
        alias_capacity: usize,
        base_map_capacity: usize,
    ) -> Self {
        let triangle_capacity = triangle_capacity.max(1);
        let alias_capacity = alias_capacity.max(1);
        let base_map_capacity = base_map_capacity.max(1);
        Self {
            triangle_lights: GfxStructuredBuffer::new_ssbo(
                resource_ctx,
                triangle_capacity,
                format!("emissive-triangle-lights-{}", frame_label),
            ),
            triangle_lights_stage: GfxStructuredBuffer::new_stage_buffer(
                resource_ctx,
                triangle_capacity,
                format!("emissive-triangle-lights-stage-{}", frame_label),
            ),
            alias_table: GfxStructuredBuffer::new_ssbo(
                resource_ctx,
                alias_capacity,
                format!("emissive-light-alias-table-{}", frame_label),
            ),
            alias_table_stage: GfxStructuredBuffer::new_stage_buffer(
                resource_ctx,
                alias_capacity,
                format!("emissive-light-alias-table-stage-{}", frame_label),
            ),
            base_map: GfxStructuredBuffer::new_ssbo(
                resource_ctx,
                base_map_capacity,
                format!("instance-emissive-triangle-base-map-{}", frame_label),
            ),
            base_map_stage: GfxStructuredBuffer::new_stage_buffer(
                resource_ctx,
                base_map_capacity,
                format!("instance-emissive-triangle-base-map-stage-{}", frame_label),
            ),
            triangle_capacity,
            alias_capacity,
            base_map_capacity,
            frame_label,
        }
    }

    fn ensure_capacity(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        required_triangles: usize,
        required_aliases: usize,
        required_base_map: usize,
    ) {
        if required_triangles <= self.triangle_capacity
            && required_aliases <= self.alias_capacity
            && required_base_map <= self.base_map_capacity
        {
            return;
        }

        let frame_label = self.frame_label;
        let triangle_capacity = next_capacity(required_triangles.max(self.triangle_capacity));
        let alias_capacity = next_capacity(required_aliases.max(self.alias_capacity));
        let base_map_capacity = next_capacity(required_base_map.max(self.base_map_capacity));

        self.destroy_mut(resource_ctx, DestroyReason::ImmediateRelease);
        *self = Self::with_capacity(resource_ctx, frame_label, triangle_capacity, alias_capacity, base_map_capacity);
    }

    fn upload(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        cmd: &GfxCommandBuffer,
        barrier_mask: GfxBarrierMask,
        triangles: &[gpu::light::EmissiveTriangleLight],
        aliases: &[gpu::light::EmissiveLightAliasEntry],
        base_map: &[u32],
    ) {
        Self::upload_structured_slice(
            resource_ctx,
            cmd,
            barrier_mask,
            &mut self.triangle_lights_stage,
            &mut self.triangle_lights,
            triangles,
        );
        Self::upload_structured_slice(
            resource_ctx,
            cmd,
            barrier_mask,
            &mut self.alias_table_stage,
            &mut self.alias_table,
            aliases,
        );
        Self::upload_structured_slice(
            resource_ctx,
            cmd,
            barrier_mask,
            &mut self.base_map_stage,
            &mut self.base_map,
            base_map,
        );
    }

    fn binding(&self, alias_count: u32, enabled: u32, version: u32, record_count: u32) -> EmissiveLightBinding {
        EmissiveLightBinding {
            triangle_lights_device_address: self.triangle_lights.device_address(),
            alias_table_device_address: self.alias_table.device_address(),
            base_map_device_address: self.base_map.device_address(),
            alias_count,
            enabled,
            version,
            record_count,
        }
    }

    fn upload_structured_slice<T: Copy>(
        resource_ctx: GfxResourceCtx<'_>,
        cmd: &GfxCommandBuffer,
        barrier_mask: GfxBarrierMask,
        stage: &mut GfxStructuredBuffer<T>,
        dst: &mut GfxStructuredBuffer<T>,
        data: &[T],
    ) {
        if data.is_empty() {
            return;
        }

        stage.mapped_slice()[..data.len()].copy_from_slice(data);
        let copy_size = std::mem::size_of_val(data) as vk::DeviceSize;
        stage.flush(resource_ctx, 0, copy_size);
        cmd.cmd_copy_buffer(
            stage,
            dst,
            &[vk::BufferCopy {
                size: copy_size,
                ..Default::default()
            }],
        );
        cmd.buffer_memory_barrier(
            vk::DependencyFlags::empty(),
            &[GfxBufferBarrier::default().mask(barrier_mask).buffer(dst.vk_buffer(), 0, copy_size)],
        );
    }

    fn destroy_mut(&mut self, resource_ctx: GfxResourceCtx<'_>, reason: DestroyReason) {
        self.triangle_lights.destroy_mut(resource_ctx, reason);
        self.triangle_lights_stage.destroy_mut(resource_ctx, reason);
        self.alias_table.destroy_mut(resource_ctx, reason);
        self.alias_table_stage.destroy_mut(resource_ctx, reason);
        self.base_map.destroy_mut(resource_ctx, reason);
        self.base_map_stage.destroy_mut(resource_ctx, reason);
    }
}

/// runtime 私有的自发光三角形 light table owner。
///
/// 它只读取 prepare 阶段已经解析好的 `RenderData` 和 `SceneReadView` 的材质 emissive view，
/// 不访问 `World` 或 pass 资源。GPU buffer 按 FIF 拆分，避免 CPU 更新当前帧表时覆盖
/// 上一帧仍可能被 raygen 读取的 table。
pub(crate) struct RenderEmissiveLightTable {
    frames: [EmissiveLightFrameBuffers; FrameCounter::fif_count()],
    triangle_lights: Vec<gpu::light::EmissiveTriangleLight>,
    alias_table: Vec<gpu::light::EmissiveLightAliasEntry>,
    base_map: Vec<u32>,
    last_revision: Option<u64>,
    version: u32,
}

impl RenderEmissiveLightTable {
    pub(crate) fn new(resource_ctx: GfxResourceCtx<'_>) -> Self {
        Self {
            frames: FrameCounter::frame_labes()
                .map(|frame_label| EmissiveLightFrameBuffers::new(resource_ctx, frame_label)),
            triangle_lights: Vec::new(),
            alias_table: Vec::new(),
            base_map: Vec::new(),
            last_revision: None,
            version: 1,
        }
    }

    pub(crate) fn update_and_upload(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        cmd: &GfxCommandBuffer,
        barrier_mask: GfxBarrierMask,
        frame_counter: &FrameCounter,
        render_data: &RenderData<'_>,
        scene: SceneReadView<'_>,
        revision: u64,
    ) -> EmissiveLightBinding {
        if self.last_revision != Some(revision) {
            self.rebuild(render_data, scene);
            self.last_revision = Some(revision);
            self.version = self.version.saturating_add(1).max(1);
        }

        let frame_label = frame_counter.frame_label();
        let frame = &mut self.frames[*frame_label];
        frame.ensure_capacity(resource_ctx, self.triangle_lights.len(), self.alias_table.len(), self.base_map.len());
        frame.upload(resource_ctx, cmd, barrier_mask, &self.triangle_lights, &self.alias_table, &self.base_map);

        let alias_count = u32::try_from(self.alias_table.len()).expect("emissive alias table exceeds u32 range");
        // record_count 与 alias_count 语义不同：alias table 可能只包含正 power 的可采样记录，
        // 但 ReSTIR key 保存的是 triangle_lights record index，重建时必须按完整 record buffer 判界。
        let record_count =
            u32::try_from(self.triangle_lights.len()).expect("emissive triangle record count exceeds u32 range");
        frame.binding(alias_count, u32::from(alias_count > 0), self.version, record_count)
    }

    pub(crate) fn destroy_mut(&mut self, resource_ctx: GfxResourceCtx<'_>) {
        for frame in &mut self.frames {
            frame.destroy_mut(resource_ctx, DestroyReason::Shutdown);
        }
    }

    fn rebuild(&mut self, render_data: &RenderData<'_>, scene: SceneReadView<'_>) {
        self.triangle_lights.clear();
        self.alias_table.clear();
        self.base_map.clear();

        let mut weighted_records = Vec::new();
        for instance in &render_data.all_instances {
            self.append_instance(render_data, scene, instance, &mut weighted_records);
        }

        let total_weight = weighted_records.iter().map(|(_, weight)| *weight).sum::<f64>();
        if total_weight <= f64::EPSILON {
            return;
        }

        for &(light_index, weight) in &weighted_records {
            self.triangle_lights[light_index].select_pdf = (weight / total_weight) as f32;
        }
        self.alias_table = Self::build_alias_table(&weighted_records, total_weight);
    }

    fn append_instance(
        &mut self,
        render_data: &RenderData<'_>,
        scene: SceneReadView<'_>,
        instance: &InstanceRenderData,
        weighted_records: &mut Vec<(usize, f64)>,
    ) {
        let mesh = &render_data.all_meshes[instance.mesh_index];
        debug_assert_eq!(
            instance.material_slots.len(),
            instance.material_handles.len(),
            "material slots and scene material handles must stay aligned"
        );
        for (submesh_idx, (&material_slot, &material_handle)) in
            instance.material_slots.iter().zip(instance.material_handles.iter()).enumerate()
        {
            let Some(material) = scene.material_emissive_view(material_handle) else {
                self.base_map.push(INVALID_EMISSIVE_TRIANGLE_BASE);
                continue;
            };
            let Some(triangles) = mesh.triangle_metadata.get(submesh_idx).map(Vec::as_slice) else {
                self.base_map.push(INVALID_EMISSIVE_TRIANGLE_BASE);
                continue;
            };
            if triangles.is_empty() || !Self::is_emissive_material(material) {
                self.base_map.push(INVALID_EMISSIVE_TRIANGLE_BASE);
                continue;
            }

            let base_index =
                u32::try_from(self.triangle_lights.len()).expect("emissive triangle record count exceeds u32 range");
            self.base_map.push(base_index);
            self.append_submesh_triangles(
                instance,
                submesh_idx,
                material_slot,
                material,
                mesh,
                triangles,
                weighted_records,
            );
        }
    }

    fn append_submesh_triangles(
        &mut self,
        instance: &InstanceRenderData,
        submesh_idx: usize,
        material_slot: u32,
        material: SceneMaterialEmissiveView<'_>,
        _mesh: &MeshRenderData<'_>,
        triangles: &[RtTriangleMeta],
        weighted_records: &mut Vec<(usize, f64)>,
    ) {
        let estimated_base_color =
            if material.diffuse_texture().is_some() { glam::Vec3::ONE } else { material.base_color().truncate() };
        let estimated_radiance = material.emissive().truncate() * estimated_base_color;
        let luminance = Self::luminance(estimated_radiance).max(0.0);

        for triangle in triangles {
            let world_positions = triangle.positions.map(|position| instance.transform.transform_point3(position));
            let normal_unnormalized =
                (world_positions[1] - world_positions[0]).cross(world_positions[2] - world_positions[0]);
            let world_area = if triangle.local_area > 0.0 { 0.5 * normal_unnormalized.length() } else { 0.0 };
            let normal = if world_area > 0.0 { normal_unnormalized.normalize() } else { glam::Vec3::Y };

            let light_index = self.triangle_lights.len();
            self.triangle_lights.push(gpu::light::EmissiveTriangleLight {
                p0: world_positions[0].into(),
                area: world_area,
                p1: world_positions[1].into(),
                select_pdf: 0.0,
                p2: world_positions[2].into(),
                material_slot,
                normal: normal.into(),
                instance_id: instance.instance_slot.as_u32(),
                uv0: triangle.uvs[0].into(),
                uv1: triangle.uvs[1].into(),
                uv2: triangle.uvs[2].into(),
                geometry_id: submesh_idx as u32,
                primitive_id: triangle.primitive_id,
            });

            let weight = f64::from(luminance) * f64::from(world_area);
            if weight > f64::EPSILON {
                weighted_records.push((light_index, weight));
            }
        }
    }

    fn is_emissive_material(material: SceneMaterialEmissiveView<'_>) -> bool {
        let emissive = material.emissive().truncate();
        emissive.max_element() > 0.0
    }

    fn luminance(color: glam::Vec3) -> f32 {
        color.dot(glam::vec3(0.2126, 0.7152, 0.0722))
    }

    fn build_alias_table(
        weighted_records: &[(usize, f64)],
        total_weight: f64,
    ) -> Vec<gpu::light::EmissiveLightAliasEntry> {
        let count = weighted_records.len();
        if count == 0 || total_weight <= f64::EPSILON {
            return Vec::new();
        }

        let mut scaled =
            weighted_records.iter().map(|(_, weight)| *weight * count as f64 / total_weight).collect::<Vec<_>>();
        let mut small = Vec::new();
        let mut large = Vec::new();
        for (idx, probability) in scaled.iter().enumerate() {
            if *probability < 1.0 {
                small.push(idx);
            } else {
                large.push(idx);
            }
        }

        let mut alias_probability = vec![1.0_f32; count];
        let mut alias_index = (0..count).collect::<Vec<_>>();
        while !small.is_empty() && !large.is_empty() {
            let small_idx = small.pop().unwrap();
            let large_idx = large.pop().unwrap();
            alias_probability[small_idx] = scaled[small_idx].clamp(0.0, 1.0) as f32;
            alias_index[small_idx] = large_idx;

            scaled[large_idx] = (scaled[large_idx] + scaled[small_idx]) - 1.0;
            if scaled[large_idx] < 1.0 {
                small.push(large_idx);
            } else {
                large.push(large_idx);
            }
        }

        for idx in small.into_iter().chain(large.into_iter()) {
            alias_probability[idx] = 1.0;
            alias_index[idx] = idx;
        }

        weighted_records
            .iter()
            .enumerate()
            .map(|(idx, (light_index, _))| {
                let alias_light_index = weighted_records[alias_index[idx]].0;
                gpu::light::EmissiveLightAliasEntry {
                    alias_probability: alias_probability[idx],
                    light_index: u32::try_from(*light_index).expect("emissive light index exceeds u32 range"),
                    alias_light_index: u32::try_from(alias_light_index)
                        .expect("emissive alias light index exceeds u32 range"),
                    _padding_0: 0,
                }
            })
            .collect()
    }
}

fn next_capacity(required: usize) -> usize {
    required.max(1).next_power_of_two()
}
