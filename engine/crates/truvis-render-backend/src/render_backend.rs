use std::ffi::CStr;

use ash::vk;
use raw_window_handle::{RawDisplayHandle, RawWindowHandle};

use truvis_asset::asset_hub::AssetHub;
use truvis_gfx::basic::bytes::BytesConvert;
use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_gfx::commands::semaphore::GfxSemaphore;
use truvis_gfx::resources::special_buffers::structured_buffer::GfxStructuredBuffer;
use truvis_gfx::swapchain::swapchain::GfxSwapchainImageInfo;
use truvis_gfx::utilities::descriptor_cursor::GfxDescriptorCursor;
use truvis_gfx::{
    commands::{
        barrier::{GfxBarrierMask, GfxBufferBarrier},
        submit_info::GfxSubmitInfo,
    },
    gfx::Gfx,
};
use truvis_render_interface::bindless_manager::BindlessManager;
use truvis_render_interface::cmd_allocator::CmdAllocator;
use truvis_render_interface::fif_buffer::FifBuffers;
use truvis_render_interface::frame_counter::FrameCounter;
use truvis_render_interface::gfx_resource_manager::GfxResourceManager;
use truvis_render_interface::global_descriptor_sets::{GlobalDescriptorSets, PerFrameDescriptorBinding};
use truvis_render_interface::gpu_scene::GpuScene;
use truvis_render_interface::pipeline_settings::{
    AccumData, DefaultRenderBackendSettings, FrameSettings, PipelineSettings,
};
use truvis_render_interface::sampler_manager::RenderSamplerManager;
use truvis_scene::scene_manager::SceneManager;
use truvis_shader_binding::gpu;

use truvis_render_interface::render_world::RenderWorld;
use truvis_world::World;

use crate::platform::camera::Camera;
use crate::platform::timer::Timer;
use crate::present::render_present::RenderPresent;

/// Rendering backend core.
///
/// Exposes state exclusively through lifecycle methods that return typed Ctx structs.
/// External code drives the lifecycle; RenderBackend does not know about Plugin,
/// GUI, or app orchestration concepts.
///
/// # Lifecycle call order
/// ```ignore
/// render_backend.begin_frame();
/// let update_ctx = render_backend.update_phase();
/// // ... use update_ctx for app/plugin CPU update ...
/// drop(update_ctx);
/// render_backend.prepare(camera);
/// let render_ctx = render_backend.render_phase();
/// // ... app/plugin render graph work ...
/// drop(render_ctx);
/// render_backend.present();
/// render_backend.end_frame();
/// ```
pub struct RenderBackend {
    world: World,
    render_world: RenderWorld,

    cmd_allocator: CmdAllocator,

    timer: Timer,
    fif_timeline_semaphore: GfxSemaphore,

    gpu_scene_update_cmds: Vec<GfxCommandBuffer>,

    render_present: Option<RenderPresent>,
}

// ---------------------------------------------------------------------------
// Lifecycle Context Types
// ---------------------------------------------------------------------------

/// Update phase context — borrows RenderBackend fields needed for CPU-side updates.
///
/// Alive while the app performs update work; RenderBackend is locked until dropped.
pub struct RenderBackendUpdateCtx<'a> {
    pub world: &'a mut World,
    pub pipeline_settings: &'a mut PipelineSettings,
    pub frame_settings: &'a FrameSettings,
    pub accum_data: &'a AccumData,
    pub swapchain_extent: vk::Extent2D,
    pub delta_time_s: f32,
}

/// Render phase context — shared (read-only) borrow of RenderBackend state for GPU command recording.
pub struct RenderBackendRenderCtx<'a> {
    pub render_world: &'a RenderWorld,
    pub render_present: &'a RenderPresent,
    pub timeline: &'a GfxSemaphore,
}

/// Init phase context — one-time setup after window/surface creation.
///
/// Does NOT contain camera; camera belongs to the concrete app.
pub struct RenderBackendInitCtx<'a> {
    pub world: &'a mut World,
    pub render_world: &'a mut RenderWorld,
    pub cmd_allocator: &'a mut CmdAllocator,
    pub swapchain_image_info: GfxSwapchainImageInfo,
    pub render_present: &'a RenderPresent,
}

/// Swapchain resize context — produced only when swapchain was actually rebuilt.
pub struct RenderBackendResizeCtx<'a> {
    pub render_world: &'a mut RenderWorld,
    pub render_present: &'a RenderPresent,
}

// new & init
impl RenderBackend {
    pub fn new(extra_instance_ext: Vec<&'static CStr>) -> Self {
        let _span = tracy_client::span!("RenderBackend::new");

        // 初始化 Gfx 全局上下文。
        Gfx::init("Truvis".to_string(), extra_instance_ext);

        let frame_settings = FrameSettings {
            color_format: vk::Format::R32G32B32A32_SFLOAT,
            depth_format: Self::get_depth_format(),
            frame_extent: vk::Extent2D {
                width: 400,
                height: 400,
            },
        };

        let timer = Timer::default();
        let accum_data = AccumData::default();
        let fif_timeline_semaphore = GfxSemaphore::new_timeline(0, "render-timeline");

        let mut gfx_resource_manager = GfxResourceManager::new();
        let mut cmd_allocator = CmdAllocator::new();

        // 初始值应该是 1，因为 timeline semaphore 初始值是 0
        let init_frame_id = 1;
        let frame_counter = FrameCounter::new(init_frame_id, 60.0);

        let mut bindless_manager = BindlessManager::new(frame_counter.frame_token());
        let scene_manager = SceneManager::new();
        let asset_hub = AssetHub::new(&mut gfx_resource_manager, &mut bindless_manager);
        let gpu_scene = GpuScene::new(&mut gfx_resource_manager, &mut bindless_manager);
        let fif_buffers =
            FifBuffers::new(&frame_settings, &mut bindless_manager, &mut gfx_resource_manager, &frame_counter);

        let render_descriptor_sets = GlobalDescriptorSets::new();
        let sampler_manager = RenderSamplerManager::new(&render_descriptor_sets);

        let per_frame_data_buffers = FrameCounter::frame_labes().map(|frame_label| {
            GfxStructuredBuffer::<gpu::PerFrameData>::new_ubo(1, format!("per-frame-data-buffer-{frame_label}"))
        });

        let cmds = FrameCounter::frame_labes()
            .into_iter()
            .map(|frame_label| cmd_allocator.alloc_command_buffer(frame_label, "gpu-scene-update"))
            .collect();

        Self {
            cmd_allocator,
            timer,
            fif_timeline_semaphore,
            gpu_scene_update_cmds: cmds,
            render_present: None,

            world: World {
                scene_manager,
                asset_hub,
            },
            render_world: RenderWorld {
                gpu_scene,
                bindless_manager,
                global_descriptor_sets: render_descriptor_sets,
                gfx_resource_manager,
                fif_buffers,
                sampler_manager,
                per_frame_data_buffers,

                frame_counter,
                frame_settings,
                pipeline_settings: PipelineSettings::default(),

                delta_time_s: 0.0,
                total_time_s: 0.0,
                accum_data,
            },
        }
    }

    /// 根据 vulkan 实例和显卡，获取合适的深度格式
    fn get_depth_format() -> vk::Format {
        Gfx::get()
            .find_supported_format(
                DefaultRenderBackendSettings::DEPTH_FORMAT_CANDIDATES,
                vk::ImageTiling::OPTIMAL,
                vk::FormatFeatureFlags::DEPTH_STENCIL_ATTACHMENT,
            )
            .first()
            .copied()
            .unwrap_or(vk::Format::UNDEFINED)
    }
}
// destroy
impl RenderBackend {
    pub fn destroy(mut self) {
        Gfx::get().wait_idel();

        if let Some(render_present) = self.render_present.take() {
            render_present.destroy(&mut self.render_world.gfx_resource_manager);
        }

        self.render_world
            .fif_buffers
            .destroy_mut(&mut self.render_world.bindless_manager, &mut self.render_world.gfx_resource_manager);
        self.world.scene_manager.destroy();
        self.world
            .asset_hub
            .destroy(&mut self.render_world.gfx_resource_manager, &mut self.render_world.bindless_manager);
        self.render_world.bindless_manager.destroy();
        self.render_world.gpu_scene.destroy();
        self.cmd_allocator.destroy();
        self.render_world.gfx_resource_manager.destroy();
        self.fif_timeline_semaphore.destroy();
        self.render_world.global_descriptor_sets.destroy();
    }
}
// ---------------------------------------------------------------------------
// Lifecycle methods (public API)
// ---------------------------------------------------------------------------
impl RenderBackend {
    /// Self-contained frame start: timer tick, FIF wait, resource cleanup, bindless advance, asset update.
    pub fn begin_frame(&mut self) {
        let _span = tracy_client::span!("RenderBackend::begin_frame");
        self.timer.tick();

        {
            let _span = tracy_client::span!("wait fif timeline");
            let current_frame_id = self.render_world.frame_counter.frame_id();
            let fif_count = FrameCounter::fif_count();
            let wait_frame_id = current_frame_id.saturating_sub(fif_count as u64);
            const WAIT_SEMAPHORE_TIMEOUT_NS: u64 = 30 * 1000 * 1000 * 1000;
            self.fif_timeline_semaphore.wait_timeline(wait_frame_id, WAIT_SEMAPHORE_TIMEOUT_NS);
        }

        {
            self.cmd_allocator.reset_frame_commands(self.render_world.frame_counter.frame_label());
            self.render_world.gfx_resource_manager.cleanup(self.render_world.frame_counter.frame_id());
        }

        self.render_world.delta_time_s = self.timer.delta_time_s();
        self.render_world.total_time_s = self.timer.total_time_s();

        let frame_token = self.render_world.frame_counter.frame_token();
        self.render_world.bindless_manager.begin_frame(frame_token);

        // Asset update internalized into begin_frame
        self.world
            .asset_hub
            .update(&mut self.render_world.gfx_resource_manager, &mut self.render_world.bindless_manager);
    }

    /// Perform internal frame-settings sync + acquire swapchain image, then return
    /// a context for external CPU-side updates.
    pub fn update_phase(&mut self) -> RenderBackendUpdateCtx<'_> {
        let _span = tracy_client::span!("RenderBackend::update_phase");

        self.update_frame_settings();
        self.acquire_image();

        RenderBackendUpdateCtx {
            world: &mut self.world,
            pipeline_settings: &mut self.render_world.pipeline_settings,
            frame_settings: &self.render_world.frame_settings,
            accum_data: &self.render_world.accum_data,
            swapchain_extent: self.render_world.frame_settings.frame_extent,
            delta_time_s: self.render_world.delta_time_s,
        }
    }

    /// Accumulate frame tracking + upload GPU scene/descriptor data.
    pub fn prepare(&mut self, camera: &Camera) {
        let _span = tracy_client::span!("RenderBackend::prepare");

        let current_camera_dir = glam::vec3(camera.euler_yaw_deg, camera.euler_pitch_deg, camera.euler_roll_deg);
        self.render_world.accum_data.update_accum_frames(current_camera_dir, camera.position);

        self.update_gpu_scene(camera);
        self.update_perframe_descriptor_set();
    }

    /// Shared borrow — RenderBackend state is read-only during render phase.
    pub fn render_phase(&self) -> RenderBackendRenderCtx<'_> {
        RenderBackendRenderCtx {
            render_world: &self.render_world,
            render_present: self.render_present.as_ref().unwrap(),
            timeline: &self.fif_timeline_semaphore,
        }
    }

    /// Submit present command.
    pub fn present(&mut self) {
        self.render_present.as_mut().unwrap().present_image();
    }

    /// Advance frame counter.
    pub fn end_frame(&mut self) {
        let _span = tracy_client::span!("RenderBackend::end_frame");
        self.render_world.frame_counter.next_frame();
    }

    /// Query whether enough time has elapsed for the next frame.
    pub fn time_to_render(&self) -> bool {
        self.render_world.frame_counter.frame_delta_time_limit_us() < self.timer.elapsed_since_tick().as_micros() as f32
    }

    /// Handle window resize. Returns `Some(ctx)` only when swapchain was actually rebuilt.
    pub fn handle_resize(&mut self, new_size: [u32; 2]) -> Option<RenderBackendResizeCtx<'_>> {
        let render_present = self.render_present.as_mut().unwrap();
        render_present.update_window_size(new_size);

        if !render_present.need_resize() {
            return None;
        }

        render_present.rebuild_after_resized(&mut self.render_world.gfx_resource_manager);

        Some(RenderBackendResizeCtx {
            render_world: &mut self.render_world,
            render_present: self.render_present.as_ref().unwrap(),
        })
    }

    /// One-time init after window/surface creation. Returns a context for plugin initialization.
    pub fn init_after_window(
        &mut self,
        raw_display_handle: RawDisplayHandle,
        raw_window_handle: RawWindowHandle,
        window_physical_size: [u32; 2],
    ) -> RenderBackendInitCtx<'_> {
        self.render_present = Some(RenderPresent::new(
            &mut self.render_world.gfx_resource_manager,
            raw_display_handle,
            raw_window_handle,
            vk::Extent2D {
                width: window_physical_size[0],
                height: window_physical_size[1],
            },
        ));

        RenderBackendInitCtx {
            world: &mut self.world,
            render_world: &mut self.render_world,
            cmd_allocator: &mut self.cmd_allocator,
            swapchain_image_info: self.render_present.as_ref().unwrap().swapchain_image_info(),
            render_present: self.render_present.as_ref().unwrap(),
        }
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------
impl RenderBackend {
    fn acquire_image(&mut self) {
        self.render_present.as_mut().unwrap().acquire_image(self.render_world.frame_counter.frame_label());
    }

    fn update_frame_settings(&mut self) {
        let swapchain_extent = self.render_present.as_ref().unwrap().swapchain.as_ref().unwrap().extent();
        if self.render_world.frame_settings.frame_extent == swapchain_extent {
            return;
        }

        if self.render_world.frame_settings.frame_extent != swapchain_extent {
            self.render_world.frame_settings.frame_extent = swapchain_extent;
            self.resize_frame_buffer(swapchain_extent);
        }
    }

    fn resize_frame_buffer(&mut self, new_extent: vk::Extent2D) {
        self.render_world.accum_data.reset();

        unsafe {
            Gfx::get().gfx_device().device_wait_idle().unwrap();
        }
        self.render_world.frame_settings.frame_extent = new_extent;

        self.render_world.fif_buffers.rebuild(
            &mut self.render_world.bindless_manager,
            &mut self.render_world.gfx_resource_manager,
            &self.render_world.frame_settings,
            &self.render_world.frame_counter,
        );
    }

    fn update_gpu_scene(&mut self, camera: &Camera) {
        let _span = tracy_client::span!("update_gpu_scene");
        let frame_extent = self.render_world.frame_settings.frame_extent;
        let frame_label = self.render_world.frame_counter.frame_label();

        let cmd = self.gpu_scene_update_cmds[*frame_label].clone();
        cmd.begin(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT, "[update-draw-buffer]stage-to-ubo");

        let transfer_barrier_mask = GfxBarrierMask {
            src_stage: vk::PipelineStageFlags2::TRANSFER,
            src_access: vk::AccessFlags2::TRANSFER_WRITE,
            dst_stage: vk::PipelineStageFlags2::VERTEX_SHADER
                | vk::PipelineStageFlags2::FRAGMENT_SHADER
                | vk::PipelineStageFlags2::RAY_TRACING_SHADER_KHR
                | vk::PipelineStageFlags2::COMPUTE_SHADER,
            dst_access: vk::AccessFlags2::SHADER_READ | vk::AccessFlags2::UNIFORM_READ,
        };

        self.render_world
            .bindless_manager
            .prepare_render_data(&self.render_world.gfx_resource_manager, &self.render_world.global_descriptor_sets);

        let scene_render_data =
            self.world.scene_manager.prepare_render_data(&self.render_world.bindless_manager, &self.world.asset_hub);
        self.render_world.gpu_scene.upload_render_data(
            &cmd,
            transfer_barrier_mask,
            &self.render_world.frame_counter,
            &scene_render_data,
            &self.render_world.bindless_manager,
        );

        let per_frame_data = {
            let view = camera.get_view_matrix();
            let projection = camera.get_projection_matrix();

            gpu::PerFrameData {
                projection: projection.into(),
                view: view.into(),
                inv_view: view.inverse().into(),
                inv_projection: projection.inverse().into(),
                camera_pos: camera.position.into(),
                camera_forward: camera.camera_forward().into(),
                time_ms: self.timer.total_time_ms(),
                delta_time_ms: self.timer.delta_time_ms(),
                frame_id: self.render_world.frame_counter.frame_id(),
                resolution: gpu::Float2 {
                    x: frame_extent.width as f32,
                    y: frame_extent.height as f32,
                },
                accum_frames: self.render_world.accum_data.accum_frames_num() as u32,
                _padding_0: Default::default(),
                _padding_1: Default::default(),
                _padding_2: Default::default(),
            }
        };
        let crt_frame_data_buffer = &self.render_world.per_frame_data_buffers[*frame_label];
        cmd.cmd_update_buffer(crt_frame_data_buffer.vk_buffer(), 0, BytesConvert::bytes_of(&per_frame_data));
        cmd.buffer_memory_barrier(
            vk::DependencyFlags::empty(),
            &[GfxBufferBarrier::default()
                .buffer(crt_frame_data_buffer.vk_buffer(), 0, vk::WHOLE_SIZE)
                .mask(transfer_barrier_mask)],
        );
        cmd.end();
        Gfx::get().gfx_queue().submit(vec![GfxSubmitInfo::new(std::slice::from_ref(&cmd))], None);
    }

    fn update_perframe_descriptor_set(&mut self) {
        let frame_label = self.render_world.frame_counter.frame_label();
        let per_frame_data_buffer = &self.render_world.per_frame_data_buffers[*frame_label];
        let gpu_scene_buffer = self.render_world.gpu_scene.scene_buffer(frame_label);
        let perframe_set = self.render_world.global_descriptor_sets.current_perframe_set(frame_label).handle();

        let perframe_data_buffer_info = vec![
            vk::DescriptorBufferInfo::default()
                .buffer(per_frame_data_buffer.vk_buffer())
                .offset(0)
                .range(vk::WHOLE_SIZE),
        ];
        let gpu_scene_buffer_info = vec![
            vk::DescriptorBufferInfo::default().buffer(gpu_scene_buffer.vk_buffer()).offset(0).range(vk::WHOLE_SIZE),
        ];

        Gfx::get().gfx_device().write_descriptor_sets(&[
            PerFrameDescriptorBinding::per_frame_data().write_buffer(perframe_set, 0, perframe_data_buffer_info),
            PerFrameDescriptorBinding::gpu_scene().write_buffer(perframe_set, 0, gpu_scene_buffer_info),
        ]);
    }
}
