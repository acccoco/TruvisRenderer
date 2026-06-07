use anyhow::{Result, bail};
use ash::vk;

use truvis_asset::handle::{AssetMaterialHandle, AssetMeshHandle};
use truvis_gfx::commands::barrier::GfxBufferBarrier;
use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_gfx::commands::command_pool::GfxCommandPool;
use truvis_gfx::commands::fence::GfxFence;
use truvis_gfx::commands::submit_info::GfxSubmitInfo;
use truvis_gfx::gfx::{GfxDeviceCtx, GfxDeviceInfoCtx, GfxQueueCtx, GfxResourceCtx};
use truvis_gfx::resources::lifecycle::DestroyReason;
use truvis_gfx::resources::special_buffers::structured_buffer::GfxStructuredBuffer;
use truvis_render_foundation::global_descriptor_sets::GlobalDescriptorSets;
use truvis_render_foundation::render_scene_view::RenderSceneView;
use truvis_render_foundation::shader_binding_system::ShaderBindingView;
use truvis_shader_binding::gpu;
use truvis_world::guid_new_type::InstanceHandle;

use crate::frame_timing::FrameTiming;
use crate::instance_bridge::InstanceBridge;

mod pass;

use self::pass::RayCastPass;

/// App 在 after_prepare 阶段提交的 world-space ray。
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

/// 已从 GPU scene slot 转回 CPU scene/asset handle 的 closest hit。
#[derive(Clone, Debug)]
pub struct RayCastHit {
    pub instance: InstanceHandle,
    pub mesh: AssetMeshHandle,
    pub material: AssetMaterialHandle,
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
/// 已上传完成的 GPU scene/TLAS，使用独立 command pool 与 fence 提交并阻塞读回，
/// 不进入 RenderGraph，避免 App 在 render graph 组图阶段之外持有 pass 资源。
pub(crate) struct RayCastService {
    pass: Option<RayCastPass>,
    command_pool: Option<GfxCommandPool>,
    fence: Option<GfxFence>,
    ray_buffer: Option<GfxStructuredBuffer<gpu::raycast::Ray>>,
    raw_hit_buffer: Option<GfxStructuredBuffer<gpu::raycast::RawHit>>,
    readback_buffer: Option<GfxStructuredBuffer<gpu::raycast::RawHit>>,
    capacity: usize,
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
        let pass = RayCastPass::new(resource_ctx, device_ctx, device_info_ctx, global_descriptor_sets);
        let command_pool = GfxCommandPool::new(
            device_ctx,
            queue_ctx.gfx_queue().queue_family().clone(),
            vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER,
            "RayCastCommandPool",
        );
        let fence = GfxFence::new(device_ctx, true, "RayCastFence");

        Self {
            pass: Some(pass),
            command_pool: Some(command_pool),
            fence: Some(fence),
            ray_buffer: None,
            raw_hit_buffer: None,
            readback_buffer: None,
            capacity: 0,
            destroyed: false,
        }
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
        instance_bridge: &InstanceBridge,
        rays: &[RayCastRay],
    ) -> Result<Vec<RayCastResult>> {
        let _span = tracy_client::span!("RayCastService::cast_sync");
        Self::validate_rays(rays)?;
        if rays.is_empty() {
            return Ok(Vec::new());
        }

        let frame_label = frame_timing.frame_label();
        let Some(tlas) = render_scene.tlas_handle(frame_label) else {
            return Ok(vec![RayCastResult::Miss; rays.len()]);
        };

        self.ensure_capacity(resource_ctx, rays.len());
        self.write_ray_buffer(resource_ctx, rays);

        let ray_bytes = (rays.len() * size_of::<gpu::raycast::Ray>()) as vk::DeviceSize;
        let raw_hit_bytes = (rays.len() * size_of::<gpu::raycast::RawHit>()) as vk::DeviceSize;
        let command_pool = self.command_pool.as_ref().expect("RayCastService command pool missing");
        let fence = self.fence.as_ref().expect("RayCastService fence missing");
        let ray_buffer = self.ray_buffer.as_ref().expect("RayCastService ray buffer missing");
        let raw_hit_buffer = self.raw_hit_buffer.as_ref().expect("RayCastService raw hit buffer missing");
        let readback_buffer = self.readback_buffer.as_ref().expect("RayCastService readback buffer missing");
        let pass = self.pass.as_ref().expect("RayCastService pass missing");

        fence.reset(device_ctx);
        let cmd = GfxCommandBuffer::new(device_ctx, command_pool, "RayCastCommand");
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

        queue_ctx.gfx_queue().submit(vec![GfxSubmitInfo::new(std::slice::from_ref(&cmd))], Some(fence.clone()));
        fence.wait(device_ctx);
        command_pool.free_command_buffers(device_ctx, vec![cmd]);

        readback_buffer.invalidate(resource_ctx, 0, raw_hit_bytes);
        self.convert_raw_hits(instance_bridge, &readback_buffer.mapped_slice_ref()[..rays.len()])
    }

    pub(crate) fn destroy_mut(&mut self, resource_ctx: GfxResourceCtx<'_>, device_ctx: GfxDeviceCtx<'_>) {
        if self.destroyed {
            return;
        }

        if let Some(mut buffer) = self.ray_buffer.take() {
            buffer.destroy_mut(resource_ctx, DestroyReason::Shutdown);
        }
        if let Some(mut buffer) = self.raw_hit_buffer.take() {
            buffer.destroy_mut(resource_ctx, DestroyReason::Shutdown);
        }
        if let Some(mut buffer) = self.readback_buffer.take() {
            buffer.destroy_mut(resource_ctx, DestroyReason::Shutdown);
        }
        if let Some(pass) = self.pass.take() {
            pass.destroy(resource_ctx, device_ctx);
        }
        if let Some(fence) = self.fence.take() {
            fence.destroy(device_ctx);
        }
        if let Some(mut command_pool) = self.command_pool.take() {
            command_pool.destroy(device_ctx);
        }
        self.destroyed = true;
    }

    fn validate_rays(rays: &[RayCastRay]) -> Result<()> {
        for (idx, ray) in rays.iter().enumerate() {
            if !ray.origin_ws.is_finite() {
                bail!("raycast ray[{idx}] origin is not finite");
            }
            if !ray.direction_ws.is_finite() || ray.direction_ws.length_squared() <= f32::EPSILON {
                bail!("raycast ray[{idx}] direction is invalid");
            }
            if !ray.t_min.is_finite() || !ray.t_max.is_finite() || ray.t_min < 0.0 || ray.t_min >= ray.t_max {
                bail!("raycast ray[{idx}] has invalid range [{}, {}]", ray.t_min, ray.t_max);
            }
        }
        Ok(())
    }

    fn ensure_capacity(&mut self, resource_ctx: GfxResourceCtx<'_>, ray_count: usize) {
        if self.capacity >= ray_count {
            return;
        }

        let new_capacity = ray_count.next_power_of_two().max(1);
        if let Some(mut buffer) = self.ray_buffer.take() {
            buffer.destroy_mut(resource_ctx, DestroyReason::ImmediateRelease);
        }
        if let Some(mut buffer) = self.raw_hit_buffer.take() {
            buffer.destroy_mut(resource_ctx, DestroyReason::ImmediateRelease);
        }
        if let Some(mut buffer) = self.readback_buffer.take() {
            buffer.destroy_mut(resource_ctx, DestroyReason::ImmediateRelease);
        }

        self.ray_buffer = Some(GfxStructuredBuffer::<gpu::raycast::Ray>::new(
            resource_ctx,
            "raycast-rays",
            new_capacity,
            vk::BufferUsageFlags::STORAGE_BUFFER,
            true,
        ));
        self.raw_hit_buffer = Some(GfxStructuredBuffer::<gpu::raycast::RawHit>::new(
            resource_ctx,
            "raycast-raw-hits",
            new_capacity,
            vk::BufferUsageFlags::STORAGE_BUFFER | vk::BufferUsageFlags::TRANSFER_SRC,
            false,
        ));
        self.readback_buffer = Some(GfxStructuredBuffer::<gpu::raycast::RawHit>::new_readback_buffer(
            resource_ctx,
            new_capacity,
            "raycast-readback",
        ));
        self.capacity = new_capacity;
    }

    fn write_ray_buffer(&mut self, resource_ctx: GfxResourceCtx<'_>, rays: &[RayCastRay]) {
        let ray_buffer = self.ray_buffer.as_mut().expect("RayCastService ray buffer missing");
        for (dst, src) in ray_buffer.mapped_slice()[..rays.len()].iter_mut().zip(rays.iter()) {
            *dst = gpu::raycast::Ray {
                origin_ws: src.origin_ws.into(),
                t_min: src.t_min,
                direction_ws: src.direction_ws.into(),
                t_max: src.t_max,
            };
        }
        ray_buffer.flush(resource_ctx, 0, (rays.len() * size_of::<gpu::raycast::Ray>()) as vk::DeviceSize);
    }

    fn convert_raw_hits(
        &self,
        instance_bridge: &InstanceBridge,
        raw_hits: &[gpu::raycast::RawHit],
    ) -> Result<Vec<RayCastResult>> {
        raw_hits
            .iter()
            .map(|raw| {
                if raw.hit == 0 {
                    return Ok(RayCastResult::Miss);
                }

                let record = instance_bridge
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
