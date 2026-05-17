use ash::vk;
use itertools::Itertools;

use truvis_gfx::gfx::{GfxDeviceCtx, GfxImmediateCtx, GfxResourceCtx};
use truvis_gfx::raytracing::acceleration::GfxAcceleration;
use truvis_gfx::resources::lifecycle::DestroyReason;
use truvis_render_interface::frame_counter::FrameCounter;

use super::gpu_scene::GpuScene;
use super::render_data::{InstanceRenderData, RenderData};

impl GpuScene {
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
        vk::AccelerationStructureInstanceKHR {
            // 3x4 row-major 矩阵
            transform: get_rt_matrix(&instance.transform),
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
            if let Some(tlas) = self.gpu_scene_buffers[frame_index].tlas.take() {
                tlas.destroy(resource_ctx, device_ctx, DestroyReason::ImmediateRelease);
            }
            self.gpu_scene_buffers[frame_index].tlas_revision = tlas_revision;
            return;
        }

        if self.gpu_scene_buffers[frame_index].tlas.is_some()
            && self.gpu_scene_buffers[frame_index].tlas_revision == tlas_revision
        {
            return;
        }

        let instance_infos = scene_data
            .all_instances
            .iter()
            .map(|ins| {
                ins.instance_slot.validate_tlas_custom_index();
                self.get_as_instance_info(ins, ins.instance_slot.as_u32(), scene_data)
            })
            .collect_vec();

        if let Some(tlas) = self.gpu_scene_buffers[frame_index].tlas.take() {
            tlas.destroy(resource_ctx, device_ctx, DestroyReason::ImmediateRelease);
        }

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
}

fn get_rt_matrix(trans: &glam::Mat4) -> vk::TransformMatrixKHR {
    let c1 = &trans.x_axis;
    let c2 = &trans.y_axis;
    let c3 = &trans.z_axis;
    let c4 = &trans.w_axis;

    // 3x4 矩阵，row-major 顺序
    vk::TransformMatrixKHR {
        matrix: [
            c1.x, c2.x, c3.x, c4.x, // 第 1 行
            c1.y, c2.y, c3.y, c4.y, // 第 2 行
            c1.z, c2.z, c3.z, c4.z, // 第 3 行
        ],
    }
}
