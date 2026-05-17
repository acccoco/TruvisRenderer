use std::ffi::CStr;

use ash::vk;
use raw_window_handle::{RawDisplayHandle, RawWindowHandle};

use truvis_asset::asset_hub::{AssetHub, LoadedAssetEvent};
use truvis_gfx::basic::bytes::BytesConvert;
use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_gfx::commands::semaphore::GfxSemaphore;
use truvis_gfx::resources::lifecycle::DestroyReason;
use truvis_gfx::resources::special_buffers::structured_buffer::GfxStructuredBuffer;
use truvis_gfx::swapchain::swapchain::GfxSwapchainImageInfo;
use truvis_gfx::utilities::descriptor_cursor::GfxDescriptorCursor;
use truvis_gfx::{
    commands::{
        barrier::{GfxBarrierMask, GfxBufferBarrier},
        submit_info::GfxSubmitInfo,
    },
    gfx::{Gfx, GfxDeviceCtx, GfxDeviceInfoCtx, GfxImmediateCtx, GfxQueueCtx, GfxResourceCtx, GfxSurfaceCtx},
};
use truvis_render_interface::bindless_manager::BindlessManager;
use truvis_render_interface::cmd_allocator::CmdAllocator;
use truvis_render_interface::fif_buffer::FifBuffers;
use truvis_render_interface::frame_counter::FrameCounter;
use truvis_render_interface::gfx_resource_manager::GfxResourceManager;
use truvis_render_interface::global_descriptor_sets::{GlobalDescriptorSets, PerFrameDescriptorBinding};
use truvis_render_interface::pipeline_settings::{
    AccumData, DefaultRenderBackendSettings, FrameSettings, PipelineSettings,
};
use truvis_render_interface::render_scene_view::RenderSceneView;
use truvis_render_interface::sampler_manager::RenderSamplerManager;
use truvis_scene::scene_manager::SceneManager;
use truvis_shader_binding::gpu;

use truvis_render_interface::render_world::RenderWorld;
use truvis_world::World;

use crate::asset_mesh_uploader::AssetMeshUploader;
use crate::asset_texture_uploader::AssetTextureUploader;
use crate::instance_bridge::InstanceBridge;
use crate::material_bridge::MaterialBridge;
use crate::platform::camera::Camera;
use crate::platform::timer::Timer;
use crate::present::render_present::RenderPresent;
use crate::render_scene::gpu_scene::GpuScene;

/// 渲染后端核心。
///
/// 只通过返回类型化 Ctx 结构的生命周期方法暴露状态。
/// 生命周期由外部代码驱动；RenderBackend 不感知 Plugin、GUI 或 app 编排概念。
///
/// 它位于 `RenderAppShell` 之下、`truvis-gfx`/`RenderWorld` 之上，是 CPU scene、
/// render-side 资产上传、GPU scene 翻译、swapchain/present 和 FIF 同步的聚合 owner。
/// 上层只能在对应阶段拿到窄化后的 Ctx，不能长期保存完整 `Gfx` 或 backend 内部字段。
///
/// # 生命周期调用顺序
/// ```ignore
/// render_backend.begin_frame();
/// let update_ctx = render_backend.update_phase();
/// // ... 使用 update_ctx 执行 app/plugin CPU 更新 ...
/// drop(update_ctx);
/// render_backend.prepare(camera);
/// let render_ctx = render_backend.render_phase();
/// // ... 执行 app/plugin render graph 工作 ...
/// drop(render_ctx);
/// render_backend.present();
/// render_backend.end_frame();
/// ```
pub struct RenderBackend {
    gfx: Gfx,

    world: World,
    render_world: RenderWorld,
    gpu_scene: GpuScene,
    asset_texture_uploader: AssetTextureUploader,
    asset_mesh_uploader: AssetMeshUploader,
    material_bridge: MaterialBridge,
    instance_bridge: InstanceBridge,

    cmd_allocator: CmdAllocator,

    timer: Timer,
    fif_timeline_semaphore: GfxSemaphore,

    gpu_scene_update_cmds: Vec<GfxCommandBuffer>,

    render_present: Option<RenderPresent>,
}

// ---------------------------------------------------------------------------
// 生命周期上下文类型
// ---------------------------------------------------------------------------

/// Update 阶段上下文，借用 CPU 端更新需要的 RenderBackend 字段。
///
/// 在 app 执行 update 工作期间保持存活；drop 前 RenderBackend 会保持借用锁定。
/// 这个阶段允许修改 `World` 与管线设置，但还没有把 CPU 语义数据翻译到 GPU scene。
pub struct RenderBackendUpdateCtx<'a> {
    pub world: &'a mut World,
    pub pipeline_settings: &'a mut PipelineSettings,
    pub frame_settings: &'a FrameSettings,
    pub accum_data: &'a AccumData,
    pub swapchain_extent: vk::Extent2D,
    pub delta_time_s: f32,
}

/// Render 阶段上下文，对 GPU 命令录制需要的 RenderBackend 状态进行只读共享借用。
///
/// 到达这个阶段时 `prepare` 已经完成 per-frame descriptor、material buffer、scene buffer、
/// TLAS 和 raster draw cache 的更新；pass 只能读取这些结果并录制命令。
pub struct RenderBackendRenderCtx<'a> {
    pub device_ctx: GfxDeviceCtx<'a>,
    pub resource_ctx: GfxResourceCtx<'a>,
    pub queue_ctx: GfxQueueCtx<'a>,
    pub device_info_ctx: GfxDeviceInfoCtx<'a>,
    pub render_world: &'a RenderWorld,
    pub render_scene: &'a dyn RenderSceneView,
    pub asset_texture_uploader: &'a AssetTextureUploader,
    pub render_present: &'a RenderPresent,
    pub timeline: &'a GfxSemaphore,
}

/// Init 阶段上下文，用于 window/surface 创建后的一次性设置。
///
/// 不包含 camera；camera 属于具体 app。
/// 这里暴露 `World`、`RenderWorld` 和 `CmdAllocator` 的可变借用，供 app/plugin 创建长期 GPU 资源；
/// 初始化完成后这些能力会重新收敛回 backend 的阶段化生命周期。
pub struct RenderBackendInitCtx<'a> {
    pub device_ctx: GfxDeviceCtx<'a>,
    pub resource_ctx: GfxResourceCtx<'a>,
    pub queue_ctx: GfxQueueCtx<'a>,
    pub device_info_ctx: GfxDeviceInfoCtx<'a>,
    pub immediate_ctx: GfxImmediateCtx<'a>,
    pub surface_ctx: GfxSurfaceCtx<'a>,
    pub world: &'a mut World,
    pub render_world: &'a mut RenderWorld,
    pub cmd_allocator: &'a mut CmdAllocator,
    pub swapchain_image_info: GfxSwapchainImageInfo,
    pub render_present: &'a RenderPresent,
}

/// Swapchain resize 上下文，仅在 swapchain 实际重建时产生。
///
/// 上层只在收到 `Some(ctx)` 时重建窗口尺寸相关资源；连续 resize 事件会在 present 层合并。
pub struct RenderBackendResizeCtx<'a> {
    pub device_ctx: GfxDeviceCtx<'a>,
    pub resource_ctx: GfxResourceCtx<'a>,
    pub immediate_ctx: GfxImmediateCtx<'a>,
    pub surface_ctx: GfxSurfaceCtx<'a>,
    pub render_world: &'a mut RenderWorld,
    pub render_present: &'a RenderPresent,
}

/// Shutdown 阶段上下文，保证 app/plugin 可在 backend 与 Gfx 存活时释放 GPU 资源。
///
/// `RenderAppShell` 会在 backend 自身销毁前把这个上下文交给 app/plugin，确保 plugin-owned
/// pipeline、buffer、descriptor 等资源仍能通过 typed Ctx 显式释放。
pub struct RenderBackendShutdownCtx<'a> {
    pub device_ctx: GfxDeviceCtx<'a>,
    pub resource_ctx: GfxResourceCtx<'a>,
    pub queue_ctx: GfxQueueCtx<'a>,
    pub immediate_ctx: GfxImmediateCtx<'a>,
    pub surface_ctx: GfxSurfaceCtx<'a>,
    pub render_world: &'a mut RenderWorld,
    pub cmd_allocator: &'a mut CmdAllocator,
}

// 创建与初始化
impl RenderBackend {
    pub fn new(extra_instance_ext: Vec<&'static CStr>) -> Self {
        let _span = tracy_client::span!("RenderBackend::new");

        let gfx = {
            let _span = tracy_client::span!("RenderBackend::new/Gfx");
            Gfx::new("Truvis".to_string(), extra_instance_ext)
        };

        let frame_settings = {
            let _span = tracy_client::span!("RenderBackend::new/frame_settings");
            FrameSettings {
                color_format: vk::Format::R32G32B32A32_SFLOAT,
                depth_format: Self::get_depth_format(gfx.device_info_ctx()),
                frame_extent: vk::Extent2D {
                    width: 400,
                    height: 400,
                },
            }
        };

        let (timer, accum_data, fif_timeline_semaphore) = {
            let _span = tracy_client::span!("RenderBackend::new/sync");
            (Timer::default(), AccumData::default(), GfxSemaphore::new_timeline(gfx.device_ctx(), 0, "render-timeline"))
        };

        let (mut gfx_resource_manager, mut cmd_allocator, frame_counter, mut bindless_manager) = {
            let _span = tracy_client::span!("RenderBackend::new/managers");
            let gfx_resource_manager = GfxResourceManager::new();
            let cmd_allocator = CmdAllocator::new(gfx.device_ctx(), gfx.device_info_ctx());

            // 初始值应该是 1，因为 timeline semaphore 初始值是 0
            let init_frame_id = 1;
            let frame_counter = FrameCounter::new(init_frame_id, 60.0);
            let bindless_manager = BindlessManager::new(frame_counter.frame_token());

            (gfx_resource_manager, cmd_allocator, frame_counter, bindless_manager)
        };

        let asset_texture_uploader = {
            let _span = tracy_client::span!("RenderBackend::new/asset_texture_uploader");
            AssetTextureUploader::new(
                gfx.resource_ctx(),
                gfx.device_ctx(),
                gfx.immediate_ctx(),
                gfx.queue_ctx(),
                &mut gfx_resource_manager,
                &mut bindless_manager,
            )
        };
        let asset_mesh_uploader = {
            let _span = tracy_client::span!("RenderBackend::new/asset_mesh_uploader");
            AssetMeshUploader::new(gfx.device_ctx(), gfx.queue_ctx())
        };
        let material_bridge = {
            let _span = tracy_client::span!("RenderBackend::new/material_bridge");
            MaterialBridge::new(gfx.resource_ctx(), frame_counter.frame_token())
        };
        let instance_bridge = {
            let _span = tracy_client::span!("RenderBackend::new/instance_bridge");
            InstanceBridge::new(frame_counter.frame_token())
        };
        let scene_manager = {
            let _span = tracy_client::span!("RenderBackend::new/scene_manager");
            SceneManager::new()
        };
        let asset_hub = {
            let _span = tracy_client::span!("RenderBackend::new/asset_hub");
            AssetHub::new()
        };
        let gpu_scene = {
            let _span = tracy_client::span!("RenderBackend::new/gpu_scene");
            GpuScene::new(
                gfx.resource_ctx(),
                gfx.device_ctx(),
                gfx.immediate_ctx(),
                &mut gfx_resource_manager,
                &mut bindless_manager,
            )
        };
        let fif_buffers = {
            let _span = tracy_client::span!("RenderBackend::new/fif_buffers");
            FifBuffers::new(
                gfx.resource_ctx(),
                gfx.device_ctx(),
                gfx.immediate_ctx(),
                &frame_settings,
                &mut bindless_manager,
                &mut gfx_resource_manager,
                &frame_counter,
            )
        };

        let render_descriptor_sets = {
            let _span = tracy_client::span!("RenderBackend::new/global_descriptors");
            GlobalDescriptorSets::new(gfx.device_ctx())
        };
        let sampler_manager = {
            let _span = tracy_client::span!("RenderBackend::new/samplers");
            RenderSamplerManager::new(gfx.device_ctx(), render_descriptor_sets.static_sampler_target())
        };

        let per_frame_data_buffers = {
            let _span = tracy_client::span!("RenderBackend::new/per_frame_data_buffers");
            FrameCounter::frame_labes().map(|frame_label| {
                GfxStructuredBuffer::<gpu::PerFrameData>::new_ubo(
                    gfx.resource_ctx(),
                    1,
                    format!("per-frame-data-buffer-{frame_label}"),
                )
            })
        };

        let cmds = {
            let _span = tracy_client::span!("RenderBackend::new/gpu_scene_update_cmds");
            FrameCounter::frame_labes()
                .into_iter()
                .map(|frame_label| {
                    cmd_allocator.alloc_command_buffer(gfx.device_ctx(), frame_label, "gpu-scene-update")
                })
                .collect()
        };

        {
            let _span = tracy_client::span!("RenderBackend::new/assemble_state");
            Self {
                gfx,
                cmd_allocator,
                timer,
                fif_timeline_semaphore,
                gpu_scene_update_cmds: cmds,
                render_present: None,

                world: World {
                    scene_manager,
                    asset_hub,
                },
                asset_texture_uploader,
                asset_mesh_uploader,
                material_bridge,
                instance_bridge,
                gpu_scene,
                render_world: RenderWorld {
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
    }

    /// 根据 vulkan 实例和显卡，获取合适的深度格式
    fn get_depth_format(ctx: GfxDeviceInfoCtx<'_>) -> vk::Format {
        ctx.find_supported_format(
            DefaultRenderBackendSettings::DEPTH_FORMAT_CANDIDATES,
            vk::ImageTiling::OPTIMAL,
            vk::FormatFeatureFlags::DEPTH_STENCIL_ATTACHMENT,
        )
        .first()
        .copied()
        .unwrap_or(vk::Format::UNDEFINED)
    }
}
// 销毁
impl RenderBackend {
    /// 等待当前 device 上已提交的 GPU 工作完成。
    pub fn wait_idle(&self) {
        self.gfx.wait_idel();
    }

    pub fn destroy(mut self) {
        self.gfx.wait_idel();

        if let Some(render_present) = self.render_present.take() {
            render_present.destroy(
                self.gfx.resource_ctx(),
                self.gfx.device_ctx(),
                self.gfx.surface_ctx(),
                &mut self.render_world.gfx_resource_manager,
            );
        }

        self.render_world.fif_buffers.destroy_mut(
            self.gfx.resource_ctx(),
            self.gfx.device_ctx(),
            &mut self.render_world.bindless_manager,
            &mut self.render_world.gfx_resource_manager,
            DestroyReason::Shutdown,
        );
        self.world.scene_manager.destroy();
        self.material_bridge.destroy(self.gfx.resource_ctx());
        self.asset_texture_uploader.destroy(
            self.gfx.resource_ctx(),
            self.gfx.device_ctx(),
            &mut self.render_world.gfx_resource_manager,
            &mut self.render_world.bindless_manager,
        );
        self.world.asset_hub.destroy();
        self.gpu_scene.destroy_mut(
            self.gfx.resource_ctx(),
            self.gfx.device_ctx(),
            &mut self.render_world.bindless_manager,
            &mut self.render_world.gfx_resource_manager,
        );
        self.asset_mesh_uploader.destroy(self.gfx.resource_ctx(), self.gfx.device_ctx());
        for buffer in &mut self.render_world.per_frame_data_buffers {
            buffer.destroy_mut(self.gfx.resource_ctx(), DestroyReason::Shutdown);
        }
        self.gpu_scene_update_cmds.clear();
        self.cmd_allocator.destroy(self.gfx.device_ctx());
        self.render_world.gfx_resource_manager.destroy(self.gfx.resource_ctx(), self.gfx.device_ctx());
        self.fif_timeline_semaphore.destroy(self.gfx.device_ctx());
        self.render_world.sampler_manager.destroy(self.gfx.device_ctx());
        self.render_world.global_descriptor_sets.destroy(self.gfx.device_ctx());
        self.gfx.destroy();
    }
}
// ---------------------------------------------------------------------------
// 生命周期方法（public API）
// ---------------------------------------------------------------------------
impl RenderBackend {
    /// 自包含的帧开始流程：timer tick、FIF 等待、资源清理、bindless 推进和资产更新。
    ///
    /// 这里是 backend 每帧唯一的资源回收入口。先等待当前 FIF 槽位不再被 GPU 使用，
    /// 再重置命令池和延迟释放队列，最后消费 `AssetHub` 的异步事件并推进上传队列。
    pub fn begin_frame(&mut self) {
        let _span = tracy_client::span!("RenderBackend::begin_frame");
        self.timer.tick();

        {
            let _span = tracy_client::span!("wait fif timeline");
            let current_frame_id = self.render_world.frame_counter.frame_id();
            let fif_count = FrameCounter::fif_count();
            let wait_frame_id = current_frame_id.saturating_sub(fif_count as u64);
            const WAIT_SEMAPHORE_TIMEOUT_NS: u64 = 30 * 1000 * 1000 * 1000;
            self.fif_timeline_semaphore.wait_timeline(self.gfx.device_ctx(), wait_frame_id, WAIT_SEMAPHORE_TIMEOUT_NS);
        }

        {
            self.cmd_allocator
                .reset_frame_commands(self.gfx.device_ctx(), self.render_world.frame_counter.frame_label());
            self.render_world.gfx_resource_manager.cleanup(
                self.gfx.resource_ctx(),
                self.gfx.device_ctx(),
                self.render_world.frame_counter.frame_id(),
            );
        }

        self.render_world.delta_time_s = self.timer.delta_time_s();
        self.render_world.total_time_s = self.timer.total_time_s();

        let frame_token = self.render_world.frame_counter.frame_token();
        self.render_world.bindless_manager.begin_frame(frame_token);
        self.material_bridge.begin_frame(frame_token);
        self.instance_bridge.begin_frame(frame_token);

        let loaded_asset_events = self.world.asset_hub.update();
        let mut texture_events = Vec::new();
        let mut mesh_events = Vec::new();
        for event in loaded_asset_events {
            match event {
                event @ (LoadedAssetEvent::TextureLoaded { .. } | LoadedAssetEvent::TextureFailed { .. }) => {
                    texture_events.push(event);
                }
                event @ LoadedAssetEvent::MeshLoaded { .. } => {
                    mesh_events.push(event);
                }
                LoadedAssetEvent::SceneLoaded { handle } => {
                    log::debug!("Scene asset {:?} CPU data is ready", handle);
                }
                LoadedAssetEvent::SceneFailed { handle, error } => {
                    log::error!("Scene asset {:?} failed to load: {}", handle, error);
                }
            }
        }
        self.asset_texture_uploader.update(
            texture_events,
            self.gfx.resource_ctx(),
            self.gfx.device_ctx(),
            self.gfx.queue_ctx(),
            &mut self.render_world.gfx_resource_manager,
            &mut self.render_world.bindless_manager,
        );
        self.asset_mesh_uploader.update(
            mesh_events,
            self.gfx.resource_ctx(),
            self.gfx.device_ctx(),
            self.gfx.queue_ctx(),
        );
    }

    /// 执行内部 frame-settings 同步并获取 swapchain image，
    /// 然后返回供外部 CPU 端更新使用的上下文。
    ///
    /// `acquire_image` 放在 update 前，保证本帧的 swapchain image、frame extent 和后续
    /// render graph 导入的 present target 指向同一个窗口状态。
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

    /// 更新累积帧跟踪，并上传 GPU scene/descriptor 数据。
    ///
    /// 这是 update 与 render 之间的语义翻译边界：App 仍拥有 camera/input state，
    /// backend 只读取 camera 快照，并把 `World`、asset/material/instance bridge 的状态整理成
    /// render pass 可读取的 `RenderSceneView`。
    pub fn prepare(&mut self, camera: &Camera) {
        let _span = tracy_client::span!("RenderBackend::prepare");

        let current_camera_dir = glam::vec3(camera.euler_yaw_deg, camera.euler_pitch_deg, camera.euler_roll_deg);
        self.render_world.accum_data.update_accum_frames(current_camera_dir, camera.position);

        self.update_gpu_scene(camera);
        self.update_perframe_descriptor_set();
    }

    /// 共享借用：render 阶段中 RenderBackend 状态只读。
    ///
    /// 这个 Ctx 面向 RenderGraph/pass 录制。它故意不暴露 `World` 的可变借用，避免 render
    /// 阶段继续改变 CPU scene，破坏 `prepare` 已经生成的 GPU scene 快照。
    pub fn render_phase(&self) -> RenderBackendRenderCtx<'_> {
        RenderBackendRenderCtx {
            device_ctx: self.gfx.device_ctx(),
            resource_ctx: self.gfx.resource_ctx(),
            queue_ctx: self.gfx.queue_ctx(),
            device_info_ctx: self.gfx.device_info_ctx(),
            render_world: &self.render_world,
            render_scene: &self.gpu_scene,
            asset_texture_uploader: &self.asset_texture_uploader,
            render_present: self.render_present.as_ref().unwrap(),
            timeline: &self.fif_timeline_semaphore,
        }
    }

    /// 提交 present 命令。
    ///
    /// 渲染命令提交由上层 render graph 完成；这里仅把当前 swapchain image 交给 present queue，
    /// 并让 present 层记录是否需要在后续帧重建 swapchain。
    pub fn present(&mut self) {
        self.render_present.as_mut().unwrap().present_image(self.gfx.surface_ctx(), self.gfx.queue_ctx());
    }

    /// 推进帧计数器。
    ///
    /// 所有依赖 `FrameCounter` 的 FIF 资源都在此之后切到下一帧标签；因此必须放在
    /// present 之后，作为本帧生命周期的最后一步。
    pub fn end_frame(&mut self) {
        let _span = tracy_client::span!("RenderBackend::end_frame");
        self.render_world.frame_counter.next_frame();
    }

    /// 查询是否已经到达下一帧的渲染时间。
    pub fn time_to_render(&self) -> bool {
        self.render_world.frame_counter.frame_delta_time_limit_us() < self.timer.elapsed_since_tick().as_micros() as f32
    }

    /// 处理窗口 resize。只有实际重建 swapchain 时才返回 `Some(ctx)`。
    pub fn handle_resize(&mut self, new_size: [u32; 2]) -> Option<RenderBackendResizeCtx<'_>> {
        let render_present = self.render_present.as_mut().unwrap();
        render_present.update_window_size(new_size);

        if !render_present.need_resize(self.gfx.surface_ctx()) {
            return None;
        }

        render_present.rebuild_after_resized(
            self.gfx.resource_ctx(),
            self.gfx.device_ctx(),
            self.gfx.surface_ctx(),
            &mut self.render_world.gfx_resource_manager,
        );

        Some(RenderBackendResizeCtx {
            device_ctx: self.gfx.device_ctx(),
            resource_ctx: self.gfx.resource_ctx(),
            immediate_ctx: self.gfx.immediate_ctx(),
            surface_ctx: self.gfx.surface_ctx(),
            render_world: &mut self.render_world,
            render_present: self.render_present.as_ref().unwrap(),
        })
    }

    pub fn shutdown_phase(&mut self) -> RenderBackendShutdownCtx<'_> {
        RenderBackendShutdownCtx {
            device_ctx: self.gfx.device_ctx(),
            resource_ctx: self.gfx.resource_ctx(),
            queue_ctx: self.gfx.queue_ctx(),
            immediate_ctx: self.gfx.immediate_ctx(),
            surface_ctx: self.gfx.surface_ctx(),
            render_world: &mut self.render_world,
            cmd_allocator: &mut self.cmd_allocator,
        }
    }

    /// window/surface 创建后的一次性初始化。返回用于 plugin 初始化的上下文。
    ///
    /// `RenderBackend::new` 不触碰窗口系统对象；surface/swapchain 必须等平台层提供 raw handle 后
    /// 才能创建。这样可以保持 backend 初始化和窗口生命周期之间的清晰边界。
    pub fn init_after_window(
        &mut self,
        raw_display_handle: RawDisplayHandle,
        raw_window_handle: RawWindowHandle,
        window_physical_size: [u32; 2],
    ) -> RenderBackendInitCtx<'_> {
        self.render_present = Some(RenderPresent::new(
            self.gfx.resource_ctx(),
            self.gfx.device_ctx(),
            self.gfx.surface_ctx(),
            &mut self.render_world.gfx_resource_manager,
            raw_display_handle,
            raw_window_handle,
            vk::Extent2D {
                width: window_physical_size[0],
                height: window_physical_size[1],
            },
        ));

        RenderBackendInitCtx {
            device_ctx: self.gfx.device_ctx(),
            resource_ctx: self.gfx.resource_ctx(),
            queue_ctx: self.gfx.queue_ctx(),
            device_info_ctx: self.gfx.device_info_ctx(),
            immediate_ctx: self.gfx.immediate_ctx(),
            surface_ctx: self.gfx.surface_ctx(),
            world: &mut self.world,
            render_world: &mut self.render_world,
            cmd_allocator: &mut self.cmd_allocator,
            swapchain_image_info: self.render_present.as_ref().unwrap().swapchain_image_info(),
            render_present: self.render_present.as_ref().unwrap(),
        }
    }
}

// ---------------------------------------------------------------------------
// 内部辅助函数
// ---------------------------------------------------------------------------
impl RenderBackend {
    fn acquire_image(&mut self) {
        self.render_present
            .as_mut()
            .unwrap()
            .acquire_image(self.gfx.surface_ctx(), self.render_world.frame_counter.frame_label());
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

        self.gfx.device_ctx().device().wait_idle();
        self.render_world.frame_settings.frame_extent = new_extent;

        self.render_world.fif_buffers.rebuild(
            self.gfx.resource_ctx(),
            self.gfx.device_ctx(),
            self.gfx.immediate_ctx(),
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

        // GPU scene 更新使用独立命令缓冲，把 material/instance/geometry/light/scene buffer
        // 的 staging copy 和 barrier 串在一起，作为 render graph 录制前的固定准备阶段。
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

        let bindless_target = self.render_world.global_descriptor_sets.bindless_target();
        self.render_world.bindless_manager.prepare_render_data(
            self.gfx.device_ctx(),
            &self.render_world.gfx_resource_manager,
            bindless_target,
        );

        self.material_bridge.sync_asset_materials(&self.world.asset_hub);
        self.material_bridge.update_textures(&self.asset_texture_uploader);
        self.material_bridge.upload(
            self.gfx.resource_ctx(),
            &cmd,
            transfer_barrier_mask,
            frame_label,
            &self.asset_texture_uploader,
        );

        let scene_render_data = self.instance_bridge.prepare_render_data(
            &self.world.scene_manager,
            &self.material_bridge,
            &self.asset_mesh_uploader,
        );
        let material_buffer_device_address = self.material_bridge.material_buffer_device_address(frame_label);
        // mesh ready 与 instance 变化都会影响 TLAS；两个 revision 合成一条 scene revision，
        // 交给 GpuScene 判断当前 FIF 的 TLAS 是否需要重建。
        let scene_revision = self.asset_mesh_uploader.ready_revision().saturating_add(self.instance_bridge.revision());
        self.gpu_scene.upload_render_data(
            self.gfx.resource_ctx(),
            self.gfx.device_ctx(),
            self.gfx.immediate_ctx(),
            &cmd,
            transfer_barrier_mask,
            &self.render_world.frame_counter,
            &scene_render_data,
            material_buffer_device_address,
            scene_revision,
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
        self.gfx.queue_ctx().gfx_queue().submit(vec![GfxSubmitInfo::new(std::slice::from_ref(&cmd))], None);
    }

    fn update_perframe_descriptor_set(&mut self) {
        let frame_label = self.render_world.frame_counter.frame_label();
        let per_frame_data_buffer = &self.render_world.per_frame_data_buffers[*frame_label];
        let gpu_scene_buffer = self.gpu_scene.scene_buffer(frame_label);
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

        self.gfx.device_ctx().device().write_descriptor_sets(&[
            PerFrameDescriptorBinding::per_frame_data().write_buffer(perframe_set, 0, perframe_data_buffer_info),
            PerFrameDescriptorBinding::gpu_scene().write_buffer(perframe_set, 0, gpu_scene_buffer_info),
        ]);
    }
}
