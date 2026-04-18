use crate::platform::camera::Camera;
use crate::platform::timer::Timer;
use crate::present::render_present::RenderPresent;
use ash::vk;
use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use std::ffi::CStr;
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
use crate::render_context::RenderContext;
use truvis_render_graph::resources::fif_buffer::FifBuffers;
use truvis_render_interface::bindless_manager::BindlessManager;
use truvis_render_interface::cmd_allocator::CmdAllocator;
use truvis_render_interface::frame_counter::FrameCounter;
use truvis_render_interface::gfx_resource_manager::GfxResourceManager;
use truvis_render_interface::global_descriptor_sets::{GlobalDescriptorSets, PerFrameDescriptorBinding};
use truvis_render_interface::gpu_scene::GpuScene;
use truvis_render_interface::pipeline_settings::{
    AccumData, DefaultRendererSettings, FrameLabel, FrameSettings, PipelineSettings,
};
use truvis_render_interface::sampler_manager::RenderSamplerManager;
use truvis_scene::scene_manager::SceneManager;
use truvis_shader_binding::gpu;

/// 渲染 Backend 核心
///
/// 聚焦 GPU backend 能力：device / swapchain / cmd / sync / submit / present、
/// GPU 数据上传执行、descriptor 更新执行。
///
/// **不**主动推进 scene / asset 的 world 更新调度决策——
/// 这些由 `FrameRuntime` 的 phase 编排驱动。
///
/// # FrameRuntime 驱动的调用顺序
/// ```ignore
/// renderer.begin_frame();          // 等待 GPU、清理 FIF 资源
/// renderer.update_assets();        // AssetHub CPU tick（由 FrameRuntime 调度）
/// renderer.update_accum_frames();  // 累积帧跟踪（由 FrameRuntime 调度）
/// renderer.before_render(camera);  // GPU scene / descriptor 上传
/// // plugin.render(...)
/// renderer.present_image();
/// renderer.end_frame();
/// ```
pub struct Renderer {
    pub render_context: RenderContext,

    pub cmd_allocator: CmdAllocator,

    pub timer: Timer,
    pub fif_timeline_semaphore: GfxSemaphore,

    gpu_scene_update_cmds: Vec<GfxCommandBuffer>,

    pub render_present: Option<RenderPresent>,
}

// new & init
impl Renderer {
    pub fn new(extra_instance_ext: Vec<&'static CStr>) -> Self {
        let _span = tracy_client::span!("Renderer::new");

        // 初始化 RenderContext 单例
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

            render_context: RenderContext {
                asset_hub,
                scene_manager,
                gpu_scene,
                fif_buffers,
                bindless_manager,
                per_frame_data_buffers,
                gfx_resource_manager,
                global_descriptor_sets: render_descriptor_sets,
                sampler_manager,

                delta_time_s: 0.0,
                total_time_s: 0.0,
                accum_data,

                frame_counter,
                frame_settings,
                pipeline_settings: PipelineSettings::default(),
            },
        }
    }

    pub fn init_after_window(
        &mut self,
        raw_display_handle: RawDisplayHandle,
        raw_window_handle: RawWindowHandle,
        window_physical_size: [u32; 2],
    ) {
        self.render_present = Some(RenderPresent::new(
            &mut self.render_context.gfx_resource_manager,
            raw_display_handle,
            raw_window_handle,
            vk::Extent2D {
                width: window_physical_size[0],
                height: window_physical_size[1],
            },
        ));
    }

    /// 根据 vulkan 实例和显卡，获取合适的深度格式
    fn get_depth_format() -> vk::Format {
        Gfx::get()
            .find_supported_format(
                DefaultRendererSettings::DEPTH_FORMAT_CANDIDATES,
                vk::ImageTiling::OPTIMAL,
                vk::FormatFeatureFlags::DEPTH_STENCIL_ATTACHMENT,
            )
            .first()
            .copied()
            .unwrap_or(vk::Format::UNDEFINED)
    }
}
// getter
impl Renderer {
    #[inline]
    pub fn swapchain_image_info(&self) -> GfxSwapchainImageInfo {
        self.render_present.as_ref().unwrap().swapchain_image_info()
    }

    #[inline]
    pub fn frame_label(&self) -> FrameLabel {
        self.render_context.frame_counter.frame_label()
    }
}
// destroy
impl Renderer {
    pub fn destroy(mut self) {
        // 在 Renderer 被销毁时，等待 Gfx 设备空闲
        Gfx::get().wait_idel();

        if let Some(render_present) = self.render_present.take() {
            render_present.destroy(&mut self.render_context.gfx_resource_manager);
        }

        self.render_context
            .fif_buffers
            .destroy_mut(&mut self.render_context.bindless_manager, &mut self.render_context.gfx_resource_manager);
        self.render_context.scene_manager.destroy();
        self.render_context
            .asset_hub
            .destroy(&mut self.render_context.gfx_resource_manager, &mut self.render_context.bindless_manager);
        self.render_context.bindless_manager.destroy();
        self.render_context.gpu_scene.destroy();
        self.cmd_allocator.destroy();
        self.render_context.gfx_resource_manager.destroy();
        self.fif_timeline_semaphore.destroy();
        self.render_context.global_descriptor_sets.destroy();
    }
}
// phase call
impl Renderer {
    /// Backend 帧起始：timer tick、等待 FIF timeline、重置命令/资源、bindless begin_frame。
    ///
    /// 注意：asset 更新（`update_assets`）已从此方法迁出，由 `FrameRuntime` 显式调度。
    pub fn begin_frame(&mut self) {
        let _span = tracy_client::span!("Renderer::begin_frame");
        self.timer.tick();

        // 等待 fif 的同一帧渲染完成
        {
            let _span = tracy_client::span!("wait fif timeline");

            let current_frame_id = self.render_context.frame_counter.frame_id();
            let fif_count = FrameCounter::fif_count();
            let wait_frame_id = current_frame_id.saturating_sub(fif_count as u64);
            const WAIT_SEMAPHORE_TIMEOUT_NS: u64 = 30 * 1000 * 1000 * 1000; // 30s
            self.fif_timeline_semaphore.wait_timeline(wait_frame_id, WAIT_SEMAPHORE_TIMEOUT_NS);
        }

        // 清理 fif 资源
        {
            self.cmd_allocator.reset_frame_commands(self.render_context.frame_counter.frame_label());
            self.render_context.gfx_resource_manager.cleanup(self.render_context.frame_counter.frame_id());
        }

        self.render_context.delta_time_s = self.timer.delta_time_s();
        self.render_context.total_time_s = self.timer.total_time_s();

        // 子系统 begin frame
        let frame_token = self.render_context.frame_counter.frame_token();
        self.render_context.bindless_manager.begin_frame(frame_token);
    }

    /// 执行 AssetHub CPU 侧增量更新。
    ///
    /// 由 `FrameRuntime` 在 begin_frame phase 中调度，不再由 `Renderer::begin_frame` 隐式触发。
    pub fn update_assets(&mut self) {
        let _span = tracy_client::span!("Renderer::update_assets");
        self.render_context
            .asset_hub
            .update(&mut self.render_context.gfx_resource_manager, &mut self.render_context.bindless_manager);
    }

    pub fn acquire_image(&mut self) {
        // swapchain image
        self.render_present.as_mut().unwrap().acquire_image(self.render_context.frame_counter.frame_label());
    }

    pub fn present_image(&mut self) {
        self.render_present.as_mut().unwrap().present_image();
    }

    pub fn end_frame(&mut self) {
        let _span = tracy_client::span!("Renderer::end_frame");

        self.render_context.frame_counter.next_frame();
    }

    pub fn time_to_render(&mut self) -> bool {
        self.render_context.frame_counter.frame_delta_time_limit_us()
            < self.timer.elapsed_since_tick().as_micros() as f32
    }

    /// 更新累积帧计数（用于渐进式渲染）。
    ///
    /// 由 `FrameRuntime` 在 prepare phase 中调度，不再由 `before_render` 隐式触发。
    pub fn update_accum_frames(&mut self, camera: &Camera) {
        let current_camera_dir = glam::vec3(camera.euler_yaw_deg, camera.euler_pitch_deg, camera.euler_roll_deg);
        self.render_context.accum_data.update_accum_frames(current_camera_dir, camera.position);
    }

    /// Backend GPU 数据准备：将 scene/per-frame 数据上传到 GPU 并更新 descriptor sets。
    ///
    /// 注意：`update_accum_frames` 已从此方法迁出，由 `FrameRuntime` 显式调度。
    pub fn before_render(&mut self, camera: &Camera) {
        let _span = tracy_client::span!("Renderer::before_render");
        self.update_gpu_scene(camera);
        self.update_perframe_descriptor_set();
    }

    #[inline]
    pub fn need_resize(&mut self) -> bool {
        self.render_present.as_mut().unwrap().need_resize()
    }

    pub fn update_frame_settings(&mut self) {
        let swapchain_extent = self.render_present.as_ref().unwrap().swapchain.as_ref().unwrap().extent();
        if self.render_context.frame_settings.frame_extent == swapchain_extent {
            return;
        }

        // 更新 frame settings
        let extent = self.render_present.as_ref().unwrap().swapchain.as_ref().unwrap().extent();

        // Renderer: Resize Framebuffer
        if self.render_context.frame_settings.frame_extent != extent {
            self.render_context.frame_settings.frame_extent = extent;
            self.resize_frame_buffer(extent);
        }
    }

    pub fn recreate_swapchain(&mut self) {
        self.render_present.as_mut().unwrap().rebuild_after_resized(&mut self.render_context.gfx_resource_manager);
    }

    pub fn resize_frame_buffer(&mut self, new_extent: vk::Extent2D) {
        let mut accum_data = self.render_context.accum_data;
        accum_data.reset();

        unsafe {
            Gfx::get().gfx_device().device_wait_idle().unwrap();
        }
        self.render_context.frame_settings.frame_extent = new_extent;

        self.render_context.fif_buffers.rebuild(
            &mut self.render_context.bindless_manager,
            &mut self.render_context.gfx_resource_manager,
            &self.render_context.frame_settings,
            &self.render_context.frame_counter,
        );
    }

    fn update_gpu_scene(&mut self, camera: &Camera) {
        let _span = tracy_client::span!("update_gpu_scene");
        let frame_extent = self.render_context.frame_settings.frame_extent;
        let frame_label = self.render_context.frame_counter.frame_label();

        // 将数据上传到 gpu buffer 中
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

        self.render_context.bindless_manager.prepare_render_data(
            &self.render_context.gfx_resource_manager,
            &self.render_context.global_descriptor_sets,
        );

        self.render_context.gpu_scene.upload_render_data(
            &cmd,
            transfer_barrier_mask,
            &self.render_context.frame_counter,
            &self
                .render_context
                .scene_manager
                .prepare_render_data(&self.render_context.bindless_manager, &self.render_context.asset_hub),
            &self.render_context.bindless_manager,
        );

        // 准备好当前帧的数据
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
                frame_id: self.render_context.frame_counter.frame_id(),
                resolution: gpu::Float2 {
                    x: frame_extent.width as f32,
                    y: frame_extent.height as f32,
                },
                accum_frames: self.render_context.accum_data.accum_frames_num() as u32,
                _padding_0: Default::default(),
                _padding_1: Default::default(),
                _padding_2: Default::default(),
            }
        };
        let crt_frame_data_buffer = &self.render_context.per_frame_data_buffers[*frame_label];
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
        let frame_label = self.render_context.frame_counter.frame_label();
        let per_frame_data_buffer = &self.render_context.per_frame_data_buffers[*frame_label];
        let gpu_scene_buffer = self.render_context.gpu_scene.scene_buffer(frame_label);
        let perframe_set = self.render_context.global_descriptor_sets.current_perframe_set(frame_label).handle();

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
