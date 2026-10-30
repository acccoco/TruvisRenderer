use anyhow::{Result, bail};
use ash::vk;

use crate::bindings::global_descriptor_sets::GlobalDescriptorSets;
use crate::bindings::shader_binding_system::ShaderBindingView;
use truvis_gfx::commands::barrier::GfxBufferBarrier;
use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_gfx::commands::command_pool::GfxCommandPool;
use truvis_gfx::commands::fence::GfxFence;
use truvis_gfx::commands::submit_info::GfxSubmitInfo;
use truvis_gfx::gfx::{GfxDeviceCtx, GfxDeviceInfoCtx, GfxQueueCtx, GfxResourceCtx};
use truvis_gfx::resources::lifecycle::DestroyReason;
use truvis_gfx::resources::special_buffers::structured_buffer::GfxStructuredBuffer;
use truvis_render_foundation::render_scene_view::RenderSceneView;
use truvis_shader_binding::gpu;
use truvis_world::guid_new_type::{MaterialAssetHandle, MeshAssetHandle, MeshInstanceHandle};

use crate::render_world::render_instance_table::RenderInstanceTable;
use crate::state::frame_timing::FrameTiming;

mod pass;

use self::pass::RayCastPass;

/// Renderer 在 after_prepare 阶段提交的 world-space ray。
#[derive(Clone, Copy, Debug)]
pub struct RayCastRay {
    pub origin_ws: glam::Vec3,
    pub direction_ws: glam::Vec3,
    pub t_min: f32,
    pub t_max: f32,
}

/// 同步 raycast 的单条结果，顺序与输入 ray 保持一致。
#[derive(Clone, Debug)]
pub enum RayCastResult {
    Miss,
    Hit(RayCastHit),
}

/// 已从 GPU scene slot 转回 CPU world handle 的 closest hit。
#[derive(Clone, Debug)]
pub struct RayCastHit {
    pub instance: MeshInstanceHandle,
    pub mesh: MeshAssetHandle,
    pub material: MaterialAssetHandle,
    pub submesh_index: u32,
    pub primitive_index: u32,
    pub position_ws: glam::Vec3,
    pub normal_ws: glam::Vec3,
    pub uv: glam::Vec2,
    pub hit_t: f32,
}

/// Runtime-owned 同步 raycast 服务。
///
/// 该服务只在 after_prepare 阶段由 `RenderRuntimeRayCastCtx` 暴露。它复用 prepare
/// 已按 graphics queue 顺序提交的 GPU scene/TLAS，使用独立 command pool 与 fence 提交并阻塞读回，
/// 不进入 RenderGraph，避免 Renderer 在 render graph 组图阶段之外持有 pass 资源。
pub(crate) struct RayCastService {
    pass: Option<RayCastPass>,
    command_pool: Option<GfxCommandPool>,
    fence: Option<GfxFence>,
    ray_buffer: Option<GfxStructuredBuffer<gpu::engine::raycast::Ray>>,
    raw_hit_buffer: Option<GfxStructuredBuffer<gpu::engine::raycast::RawHit>>,
    readback_buffer: Option<GfxStructuredBuffer<gpu::engine::raycast::RawHit>>,
    capacity: usize,
    /// 单维 [ray_count, 1, 1] 派发同时受总 invocation 数和 X 维度上限约束。
    max_ray_count: u32,

    /// 进入 submit 前移交，fence 等待成功后取走；submit/wait 异常时禁止复用查询资源。
    /// command 的 native 分配仍由 command_pool 持有，未知完成只能在 device idle 后关闭。
    pending_command: Option<GfxCommandBuffer>,
    destroyed: bool,
}

impl RayCastService {
    pub(crate) fn new(
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        device_info_ctx: GfxDeviceInfoCtx<'_>,
        queue_ctx: GfxQueueCtx<'_>,
        global_descriptor_sets: &GlobalDescriptorSets,
    ) -> Self {
        let max_ray_count = device_info_ctx.max_ray_dispatch_width();
        let mut service = Self {
            pass: None,
            command_pool: None,
            fence: None,
            ray_buffer: None,
            raw_hit_buffer: None,
            readback_buffer: None,
            capacity: 0,
            max_ray_count,
            pending_command: None,
            destroyed: false,
        };
        service.pass = Some(RayCastPass::new(resource_ctx, device_ctx, device_info_ctx, global_descriptor_sets));
        service.command_pool = Some(GfxCommandPool::new(
            device_ctx,
            queue_ctx.gfx_queue().queue_family().clone(),
            vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER,
            "RayCastCommandPool",
        ));
        service.fence = Some(GfxFence::new(device_ctx, true, "RayCastFence"));

        service
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn cast_sync(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        queue_ctx: GfxQueueCtx<'_>,
        frame_timing: &FrameTiming,
        shader_bindings: ShaderBindingView<'_>,
        render_scene: &dyn RenderSceneView,
        render_instance_table: &RenderInstanceTable,
        rays: &[RayCastRay],
    ) -> Result<Vec<RayCastResult>> {
        let _span = tracy_client::span!("RayCastService::cast_sync");
        assert!(!self.destroyed, "RayCastService has been destroyed");
        assert!(self.pending_command.is_none(), "previous raycast submission has unknown completion");
        self.validate_rays(rays)?;
        if rays.is_empty() {
            return Ok(Vec::new());
        }

        let frame_label = frame_timing.frame_label();
        let Some(tlas) = render_scene.tlas_handle(frame_label) else {
            return Ok(vec![RayCastResult::Miss; rays.len()]);
        };

        self.ensure_capacity(resource_ctx, rays.len());
        self.write_ray_buffer(resource_ctx, rays);

        let ray_bytes = (rays.len() * size_of::<gpu::engine::raycast::Ray>()) as vk::DeviceSize;
        let raw_hit_bytes = (rays.len() * size_of::<gpu::engine::raycast::RawHit>()) as vk::DeviceSize;
        let command_pool = self.command_pool.as_ref().expect("RayCastService command pool missing");
        let fence = self.fence.as_ref().expect("RayCastService fence missing");
        let ray_buffer = self.ray_buffer.as_ref().expect("RayCastService ray buffer missing");
        let raw_hit_buffer = self.raw_hit_buffer.as_ref().expect("RayCastService raw hit buffer missing");
        let readback_buffer = self.readback_buffer.as_ref().expect("RayCastService readback buffer missing");
        let pass = self.pass.as_ref().expect("RayCastService pass missing");

        let cmd = GfxCommandBuffer::new(device_ctx, command_pool, "RayCastCommand");
        let prepared = {
            cmd.begin(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT, "RayCast");
            cmd.buffer_memory_barrier(
                vk::DependencyFlags::empty(),
                &[GfxBufferBarrier::new()
                    .buffer(ray_buffer.vk_buffer(), 0, ray_bytes)
                    .src_mask(vk::PipelineStageFlags2::HOST, vk::AccessFlags2::HOST_WRITE)
                    .dst_mask(vk::PipelineStageFlags2::RAY_TRACING_SHADER_KHR, vk::AccessFlags2::SHADER_READ)],
            );
            pass.trace(
                frame_timing,
                shader_bindings,
                tlas,
                &cmd,
                ray_buffer.vk_buffer(),
                raw_hit_buffer.vk_buffer(),
                rays.len() as u32,
            );
            cmd.buffer_memory_barrier(
                vk::DependencyFlags::empty(),
                &[GfxBufferBarrier::new()
                    .buffer(raw_hit_buffer.vk_buffer(), 0, raw_hit_bytes)
                    .src_mask(vk::PipelineStageFlags2::RAY_TRACING_SHADER_KHR, vk::AccessFlags2::SHADER_WRITE)
                    .dst_mask(vk::PipelineStageFlags2::TRANSFER, vk::AccessFlags2::TRANSFER_READ)],
            );
            cmd.cmd_copy_buffer(
                raw_hit_buffer,
                readback_buffer,
                &[vk::BufferCopy {
                    size: raw_hit_bytes,
                    ..Default::default()
                }],
            );
            cmd.buffer_memory_barrier(
                vk::DependencyFlags::empty(),
                &[GfxBufferBarrier::new()
                    .buffer(readback_buffer.vk_buffer(), 0, raw_hit_bytes)
                    .src_mask(vk::PipelineStageFlags2::TRANSFER, vk::AccessFlags2::TRANSFER_WRITE)
                    .dst_mask(vk::PipelineStageFlags2::HOST, vk::AccessFlags2::HOST_READ)],
            );
            cmd.end();
            let batches = vec![GfxSubmitInfo::new(std::slice::from_ref(&cmd))];
            fence.reset(device_ctx);
            batches
        };
        let batches = prepared;
        self.pending_command = Some(cmd);
        queue_ctx.gfx_queue().submit(batches, Some(fence));
        fence.wait(device_ctx);
        let cmd = self.pending_command.take().expect("raycast pending command missing");
        command_pool.free_command_buffers(device_ctx, vec![cmd]);

        readback_buffer.invalidate(resource_ctx, 0, raw_hit_bytes);
        self.convert_raw_hits(render_instance_table, &readback_buffer.mapped_slice_ref()[..rays.len()])
    }

    /// 调用方须先确认 device idle；这里只收尾已无 GPU 使用的资源，不代替提交/等待判定。
    /// 按依赖顺序释放后标记关闭；正常完成后允许重复关闭，panic 不保证继续回收。
    pub(crate) fn destroy_mut(&mut self, resource_ctx: GfxResourceCtx<'_>, device_ctx: GfxDeviceCtx<'_>) {
        if self.destroyed {
            return;
        }
        // 调用方已证明 device idle；丢弃命令 alias 后，native 分配随 pool 一并释放。
        self.pending_command.take();
        Self::release_buffers(
            resource_ctx,
            self.ray_buffer.take(),
            self.raw_hit_buffer.take(),
            self.readback_buffer.take(),
            DestroyReason::Shutdown,
        );
        {
            if let Some(pass) = self.pass.take() {
                pass.destroy(resource_ctx, device_ctx);
            }
        };
        {
            if let Some(fence) = self.fence.take() {
                fence.destroy(device_ctx);
            }
        };
        {
            if let Some(mut command_pool) = self.command_pool.take() {
                command_pool.destroy(device_ctx);
            }
        };
        self.capacity = 0;
        self.destroyed = true;
    }

    fn validate_rays(&self, rays: &[RayCastRay]) -> Result<()> {
        if rays.len() > self.max_ray_count as usize {
            bail!("raycast count {} exceeds device dispatch limit {}", rays.len(), self.max_ray_count);
        }
        for (idx, ray) in rays.iter().enumerate() {
            if !ray.origin_ws.is_finite() {
                bail!("raycast ray[{idx}] origin is not finite");
            }
            let direction_length_squared = ray.direction_ws.length_squared();
            if !ray.direction_ws.is_finite()
                || !direction_length_squared.is_finite()
                || direction_length_squared <= f32::EPSILON
            {
                bail!("raycast ray[{idx}] direction is invalid");
            }
            if !ray.t_min.is_finite() || !ray.t_max.is_finite() || ray.t_min < 0.0 || ray.t_min >= ray.t_max {
                bail!("raycast ray[{idx}] has invalid range [{}, {}]", ray.t_min, ray.t_max);
            }
        }
        Ok(())
    }

    /// 只在上一笔同步查询已完成时扩容；全部候选创建成功后再替换旧组合。
    /// 新容量与三个 owner 一起登记，旧资源释放诊断不能使下一次查询退回不完整状态。
    fn ensure_capacity(&mut self, resource_ctx: GfxResourceCtx<'_>, ray_count: usize) {
        assert!(!self.destroyed, "RayCastService has been destroyed");
        u32::try_from(ray_count).expect("raycast count exceeds u32 range");
        if self.capacity >= ray_count {
            return;
        }
        let new_capacity = ray_count.checked_next_power_of_two().expect("raycast capacity overflow");
        let ray_buffer;
        let raw_hit_buffer;
        let readback_buffer;
        ray_buffer = Some(GfxStructuredBuffer::<gpu::engine::raycast::Ray>::new(
            resource_ctx,
            "raycast-rays",
            new_capacity,
            vk::BufferUsageFlags::STORAGE_BUFFER,
            true,
        ));
        raw_hit_buffer = Some(GfxStructuredBuffer::<gpu::engine::raycast::RawHit>::new(
            resource_ctx,
            "raycast-raw-hits",
            new_capacity,
            vk::BufferUsageFlags::STORAGE_BUFFER | vk::BufferUsageFlags::TRANSFER_SRC,
            false,
        ));
        readback_buffer = Some(GfxStructuredBuffer::<gpu::engine::raycast::RawHit>::new_readback_buffer(
            resource_ctx,
            new_capacity,
            "raycast-readback",
        ));

        let old_ray_buffer = std::mem::replace(&mut self.ray_buffer, ray_buffer);
        let old_raw_hit_buffer = std::mem::replace(&mut self.raw_hit_buffer, raw_hit_buffer);
        let old_readback_buffer = std::mem::replace(&mut self.readback_buffer, readback_buffer);
        self.capacity = new_capacity;
        Self::release_buffers(
            resource_ctx,
            old_ray_buffer,
            old_raw_hit_buffer,
            old_readback_buffer,
            DestroyReason::ImmediateRelease,
        );
    }

    /// 三个查询 buffer 由同一 owner 在替换旧资源或已知 idle 的关闭阶段释放。
    fn release_buffers(
        resource_ctx: GfxResourceCtx<'_>,
        ray_buffer: Option<GfxStructuredBuffer<gpu::engine::raycast::Ray>>,
        raw_hit_buffer: Option<GfxStructuredBuffer<gpu::engine::raycast::RawHit>>,
        readback_buffer: Option<GfxStructuredBuffer<gpu::engine::raycast::RawHit>>,
        reason: DestroyReason,
    ) {
        if let Some(mut buffer) = ray_buffer {
            buffer.destroy_mut(resource_ctx, reason);
        }
        for mut buffer in [raw_hit_buffer, readback_buffer].into_iter().flatten() {
            buffer.destroy_mut(resource_ctx, reason);
        }
    }

    fn write_ray_buffer(&mut self, resource_ctx: GfxResourceCtx<'_>, rays: &[RayCastRay]) {
        let ray_buffer = self.ray_buffer.as_mut().expect("RayCastService ray buffer missing");
        for (dst, src) in ray_buffer.mapped_slice()[..rays.len()].iter_mut().zip(rays.iter()) {
            *dst = gpu::engine::raycast::Ray {
                origin_ws: src.origin_ws.into(),
                t_min: src.t_min,
                direction_ws: src.direction_ws.into(),
                t_max: src.t_max,
            };
        }
        ray_buffer.flush(resource_ctx, 0, (rays.len() * size_of::<gpu::engine::raycast::Ray>()) as vk::DeviceSize);
    }

    fn convert_raw_hits(
        &self,
        render_instance_table: &RenderInstanceTable,
        raw_hits: &[gpu::engine::raycast::RawHit],
    ) -> Result<Vec<RayCastResult>> {
        raw_hits
            .iter()
            .map(|raw| {
                if raw.hit == 0 {
                    return Ok(RayCastResult::Miss);
                }

                let record = render_instance_table
                    .ray_cast_record(raw.instance_slot)
                    .ok_or_else(|| anyhow::anyhow!("raycast hit unknown instance slot {}", raw.instance_slot))?;
                let material = record.materials.get(raw.submesh_index as usize).copied().ok_or_else(|| {
                    anyhow::anyhow!(
                        "raycast hit invalid submesh {} for instance slot {}",
                        raw.submesh_index,
                        raw.instance_slot
                    )
                })?;

                Ok(RayCastResult::Hit(RayCastHit {
                    instance: record.instance,
                    mesh: record.mesh,
                    material,
                    submesh_index: raw.submesh_index,
                    primitive_index: raw.primitive_index,
                    position_ws: float3_to_vec3(raw.position_ws),
                    normal_ws: float3_to_vec3(raw.normal_ws),
                    uv: float2_to_vec2(raw.uv),
                    hit_t: raw.hit_t,
                }))
            })
            .collect()
    }
}

impl Drop for RayCastService {
    fn drop(&mut self) {
        debug_assert!(self.destroyed, "RayCastService dropped without explicit destroy");
    }
}

fn float2_to_vec2(value: gpu::Float2) -> glam::Vec2 {
    glam::vec2(value.x, value.y)
}

fn float3_to_vec3(value: gpu::Float3) -> glam::Vec3 {
    glam::vec3(value.x, value.y, value.z)
}
