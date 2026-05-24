use std::ffi::CStr;

use ash::vk;
use raw_window_handle::{RawDisplayHandle, RawWindowHandle};

use truvis_asset::asset_hub::{AssetHub, AssetLoadedEvent};
use truvis_gfx::basic::bytes::BytesConvert;
use truvis_gfx::commands::barrier::{GfxBarrierMask, GfxBufferBarrier};
use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_gfx::commands::semaphore::GfxSemaphore;
use truvis_gfx::commands::submit_info::GfxSubmitInfo;
use truvis_gfx::gfx::{Gfx, GfxDeviceInfoCtx};
use truvis_gfx::resources::lifecycle::DestroyReason;
use truvis_gfx::resources::special_buffers::structured_buffer::GfxStructuredBuffer;
use truvis_gfx::utilities::descriptor_cursor::GfxDescriptorCursor;
use truvis_render_foundation::bindless_manager::BindlessManager;
use truvis_render_foundation::cmd_allocator::CmdAllocator;
use truvis_render_foundation::fif_buffer::FifBuffers;
use truvis_render_foundation::frame_counter::FrameCounter;
use truvis_render_foundation::gfx_resource_manager::GfxResourceManager;
use truvis_render_foundation::global_descriptor_sets::{GlobalDescriptorSets, PerFrameDescriptorBinding};
use truvis_render_foundation::pipeline_settings::{
    AccumData, DefaultRenderRuntimeSettings, FrameSettings, PipelineSettings,
};
use truvis_render_foundation::sampler_manager::RenderSamplerManager;
use truvis_shader_binding::gpu;
use truvis_world::scene_manager::SceneManager;

use truvis_render_foundation::gpu_store::GpuStore;
use truvis_world::World;

use crate::asset_mesh_manager::AssetMeshManager;
use crate::asset_texture_manager::AssetTextureManager;
use crate::frame_timer::FrameTimer;
use crate::instance_bridge::InstanceBridge;
use crate::material_bridge::MaterialBridge;
use crate::platform::camera::Camera;
use crate::present::render_present::RenderPresent;
use crate::ray_cast::RayCastService;
use crate::render_scene::gpu_scene::GpuScene;

pub use crate::render_runtime_ctx::{
    RenderRuntimeInitCtx, RenderRuntimeRayCastCtx, RenderRuntimeRenderCtx, RenderRuntimeResizeCtx,
    RenderRuntimeShutdownCtx, RenderRuntimeUpdateCtx,
};

/// 渲染运行时核心。
///
/// 只通过返回类型化 Ctx 结构的生命周期方法暴露状态。
/// 生命周期由外部代码驱动；RenderRuntime 不感知 Plugin、GUI 或 app 编排概念。
///
/// 它位于 `RenderAppShell` 之下、`truvis-gfx`/`GpuStore` 之上，是 CPU scene、
/// render-side 资产上传、GPU scene 翻译、swapchain/present 和 FIF 同步的聚合 owner。
/// 上层只能在对应阶段拿到窄化后的 Ctx，不能长期保存完整 `Gfx` 或 runtime 内部字段。
/// 这保证资源销毁顺序仍由 runtime 集中控制：plugin/app 可以在生命周期阶段创建或释放资源，
/// 但不能越过 Ctx 长期持有内部 owner。
///
/// # 生命周期调用顺序
/// ```ignore
/// render_runtime.begin_frame();
/// let update_ctx = render_runtime.update_phase();
/// // ... 使用 update_ctx 执行 app/plugin CPU 更新 ...
/// drop(update_ctx);
/// render_runtime.prepare(camera);
/// let render_ctx = render_runtime.render_phase();
/// // ... 执行 app/plugin render graph 工作 ...
/// drop(render_ctx);
/// render_runtime.present();
/// render_runtime.end_frame();
/// ```
pub struct RenderRuntime {
    gfx: Gfx,

    world: World,
    gpu_store: GpuStore,
    gpu_scene: GpuScene,
    asset_texture_manager: AssetTextureManager,
    asset_mesh_manager: AssetMeshManager,
    material_bridge: MaterialBridge,
    instance_bridge: InstanceBridge,
    ray_cast_service: RayCastService,

    cmd_allocator: CmdAllocator,

    timer: FrameTimer,
    fif_timeline_semaphore: GfxSemaphore,

    gpu_scene_update_cmds: Vec<GfxCommandBuffer>,

    render_present: Option<RenderPresent>,
}

// 创建与初始化
impl RenderRuntime {
    /// 创建不依赖窗口系统的 runtime root state。
    ///
    /// 这里会初始化 `Gfx`、CPU `World`、GPU `GpuStore`、资产管理器、material/instance bridge、
    /// 私有 `GpuScene`、FIF 资源和全局描述符，但不会创建 surface/swapchain。窗口相关资源必须等
    /// `init_after_window` 收到平台层 raw handle 后再创建。
    pub fn new(extra_instance_ext: Vec<&'static CStr>) -> Self {
        let _span = tracy_client::span!("RenderRuntime::new");

        let gfx = {
            let _span = tracy_client::span!("RenderRuntime::new/Gfx");
            Gfx::new("Truvis".to_string(), extra_instance_ext)
        };

        let frame_settings = {
            let _span = tracy_client::span!("RenderRuntime::new/frame_settings");
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
            let _span = tracy_client::span!("RenderRuntime::new/sync");
            (
                FrameTimer::default(),
                AccumData::default(),
                GfxSemaphore::new_timeline(gfx.device_ctx(), 0, "render-timeline"),
            )
        };

        let (mut gfx_resource_manager, mut cmd_allocator, frame_counter, mut bindless_manager) = {
            let _span = tracy_client::span!("RenderRuntime::new/managers");
            let gfx_resource_manager = GfxResourceManager::new();
            let cmd_allocator = CmdAllocator::new(gfx.device_ctx(), gfx.device_info_ctx());

            // 初始值应该是 1，因为 timeline semaphore 初始值是 0
            let init_frame_id = 1;
            let frame_counter = FrameCounter::new(init_frame_id, 60.0);
            let bindless_manager = BindlessManager::new(frame_counter.frame_token());

            (gfx_resource_manager, cmd_allocator, frame_counter, bindless_manager)
        };

        let asset_texture_manager = {
            let _span = tracy_client::span!("RenderRuntime::new/asset_texture_manager");
            AssetTextureManager::new(
                gfx.resource_ctx(),
                gfx.device_ctx(),
                gfx.immediate_ctx(),
                gfx.queue_ctx(),
                &mut gfx_resource_manager,
                &mut bindless_manager,
            )
        };
        let asset_mesh_manager = {
            let _span = tracy_client::span!("RenderRuntime::new/asset_mesh_manager");
            AssetMeshManager::new(gfx.device_ctx(), gfx.queue_ctx())
        };
        let material_bridge = {
            let _span = tracy_client::span!("RenderRuntime::new/material_bridge");
            MaterialBridge::new(gfx.resource_ctx(), frame_counter.frame_token())
        };
        let instance_bridge = {
            let _span = tracy_client::span!("RenderRuntime::new/instance_bridge");
            InstanceBridge::new(frame_counter.frame_token())
        };
        let scene_manager = {
            let _span = tracy_client::span!("RenderRuntime::new/scene_manager");
            SceneManager::new()
        };
        let asset_hub = {
            let _span = tracy_client::span!("RenderRuntime::new/asset_hub");
            AssetHub::new()
        };
        let gpu_scene = {
            let _span = tracy_client::span!("RenderRuntime::new/gpu_scene");
            GpuScene::new(
                gfx.resource_ctx(),
                gfx.device_ctx(),
                gfx.immediate_ctx(),
                &mut gfx_resource_manager,
                &mut bindless_manager,
            )
        };
        let fif_buffers = {
            let _span = tracy_client::span!("RenderRuntime::new/fif_buffers");
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
            let _span = tracy_client::span!("RenderRuntime::new/global_descriptors");
            GlobalDescriptorSets::new(gfx.device_ctx())
        };
        let sampler_manager = {
            let _span = tracy_client::span!("RenderRuntime::new/samplers");
            RenderSamplerManager::new(gfx.device_ctx(), render_descriptor_sets.static_sampler_target())
        };
        let ray_cast_service = {
            let _span = tracy_client::span!("RenderRuntime::new/ray_cast_service");
            RayCastService::new(
                gfx.resource_ctx(),
                gfx.device_ctx(),
                gfx.device_info_ctx(),
                gfx.queue_ctx(),
                &render_descriptor_sets,
            )
        };

        let per_frame_data_buffers = {
            let _span = tracy_client::span!("RenderRuntime::new/per_frame_data_buffers");
            FrameCounter::frame_labes().map(|frame_label| {
                GfxStructuredBuffer::<gpu::PerFrameData>::new_ubo(
                    gfx.resource_ctx(),
                    1,
                    format!("per-frame-data-buffer-{frame_label}"),
                )
            })
        };

        let cmds = {
            let _span = tracy_client::span!("RenderRuntime::new/gpu_scene_update_cmds");
            FrameCounter::frame_labes()
                .into_iter()
                .map(|frame_label| {
                    cmd_allocator.alloc_command_buffer(gfx.device_ctx(), frame_label, "gpu-scene-update")
                })
                .collect()
        };

        {
            let _span = tracy_client::span!("RenderRuntime::new/assemble_state");
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
                asset_texture_manager,
                asset_mesh_manager,
                material_bridge,
                instance_bridge,
                ray_cast_service,
                gpu_scene,
                gpu_store: GpuStore {
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
            DefaultRenderRuntimeSettings::DEPTH_FORMAT_CANDIDATES,
            vk::ImageTiling::OPTIMAL,
            vk::FormatFeatureFlags::DEPTH_STENCIL_ATTACHMENT,
        )
        .first()
        .copied()
        .unwrap_or(vk::Format::UNDEFINED)
    }
}
// 销毁
impl RenderRuntime {
    /// 等待当前 device 上已提交的 GPU 工作完成。
    ///
    /// runtime 在 app/plugin shutdown 前调用它，确保上层持有的 pipeline、descriptor、buffer
    /// 等资源被释放时，不会仍被上一帧 command buffer 引用。
    pub fn wait_idle(&self) {
        self.gfx.wait_idel();
    }

    /// 销毁 runtime 拥有的所有 GPU/CPU 子资源，并最后销毁 `Gfx` root owner。
    ///
    /// 调用前应已经完成 app/plugin shutdown。销毁顺序刻意从依赖 `Gfx` 的子资源开始，
    /// 先释放 present/FIF/asset/GpuScene/command/descriptor 等对象，最后销毁 `Gfx`，
    /// 这样所有 Vulkan wrapper 都能通过有效的 typed Ctx 显式释放。
    pub fn destroy(mut self) {
        self.gfx.wait_idel();

        // present 持有 surface/swapchain 与 WSI image wrapper，必须先释放；后续 FIF 和 scene
        // 资源销毁不再需要访问当前窗口 target。
        if let Some(render_present) = self.render_present.take() {
            render_present.destroy(
                self.gfx.resource_ctx(),
                self.gfx.device_ctx(),
                self.gfx.surface_ctx(),
                &mut self.gpu_store.gfx_resource_manager,
            );
        }

        self.ray_cast_service.destroy_mut(self.gfx.resource_ctx(), self.gfx.device_ctx());
        // FIF render targets 和 bindless/resource manager 存在交叉引用，统一从 GpuStore
        // 的 owner 侧释放，保证 view/bindless 句柄先退出全局表。
        self.gpu_store.fif_buffers.destroy_mut(
            self.gfx.resource_ctx(),
            self.gfx.device_ctx(),
            &mut self.gpu_store.bindless_manager,
            &mut self.gpu_store.gfx_resource_manager,
            DestroyReason::Shutdown,
        );
        // CPU scene/asset 与 render-side bridge 按依赖方向释放：先停止 scene runtime，
        // 再释放 material/texture/mesh/GpuScene 等 GPU 翻译缓存。
        self.world.scene_manager.destroy();
        self.material_bridge.destroy(self.gfx.resource_ctx());
        self.asset_texture_manager.destroy(
            self.gfx.resource_ctx(),
            self.gfx.device_ctx(),
            &mut self.gpu_store.gfx_resource_manager,
            &mut self.gpu_store.bindless_manager,
        );
        self.world.asset_hub.destroy();
        self.gpu_scene.destroy_mut(
            self.gfx.resource_ctx(),
            self.gfx.device_ctx(),
            &mut self.gpu_store.bindless_manager,
            &mut self.gpu_store.gfx_resource_manager,
        );
        self.asset_mesh_manager.destroy(self.gfx.resource_ctx(), self.gfx.device_ctx());
        // per-frame UBO 与 command allocator 在所有使用它们的 scene/present 资源之后释放。
        for buffer in &mut self.gpu_store.per_frame_data_buffers {
            buffer.destroy_mut(self.gfx.resource_ctx(), DestroyReason::Shutdown);
        }
        self.gpu_scene_update_cmds.clear();
        self.cmd_allocator.destroy(self.gfx.device_ctx());
        self.gpu_store.gfx_resource_manager.destroy(self.gfx.resource_ctx(), self.gfx.device_ctx());
        self.fif_timeline_semaphore.destroy(self.gfx.device_ctx());
        // descriptor/sampler 依赖 device 但不依赖业务资源，放在资源管理器之后、Gfx 之前销毁。
        self.gpu_store.sampler_manager.destroy(self.gfx.device_ctx());
        self.gpu_store.global_descriptor_sets.destroy(self.gfx.device_ctx());
        self.gfx.destroy();
    }
}
// ---------------------------------------------------------------------------
// 生命周期方法（public API）
// ---------------------------------------------------------------------------
impl RenderRuntime {
    /// 自包含的帧开始流程：帧计时器推进、FIF 等待、资源清理、bindless 推进和资产更新。
    ///
    /// 这里是 runtime 每帧唯一的资源回收入口。先等待当前 FIF 槽位不再被 GPU 使用，
    /// 再重置命令池和延迟释放队列，最后消费 `AssetHub` 的异步事件并推进上传队列。
    pub fn begin_frame(&mut self) {
        let _span = tracy_client::span!("RenderRuntime::begin_frame");
        self.timer.tick();

        {
            let _span = tracy_client::span!("wait fif timeline");
            let current_frame_id = self.gpu_store.frame_counter.frame_id();
            let fif_count = FrameCounter::fif_count();
            let wait_frame_id = current_frame_id.saturating_sub(fif_count as u64);
            const WAIT_SEMAPHORE_TIMEOUT_NS: u64 = 30 * 1000 * 1000 * 1000;
            // 等待当前 frame label 上一次被使用的提交完成。这个等待是后续 reset command pool、
            // immediate release 和延迟释放队列清理的安全前提。
            self.fif_timeline_semaphore.wait_timeline(self.gfx.device_ctx(), wait_frame_id, WAIT_SEMAPHORE_TIMEOUT_NS);
        }

        {
            // command allocator 和 resource manager 都以 frame label/frame id 作为回收边界；
            // 上面的 timeline wait 确保不会重置 GPU 仍在读取的命令池或资源。
            self.cmd_allocator.reset_frame_commands(self.gfx.device_ctx(), self.gpu_store.frame_counter.frame_label());
            self.gpu_store.gfx_resource_manager.cleanup(
                self.gfx.resource_ctx(),
                self.gfx.device_ctx(),
                self.gpu_store.frame_counter.frame_id(),
            );
        }

        self.gpu_store.delta_time_s = self.timer.delta_time_s();
        self.gpu_store.total_time_s = self.timer.total_time_s();

        let frame_token = self.gpu_store.frame_counter.frame_token();
        // bindless/material/instance 都使用同一个 frame token 推进延迟回收窗口，
        // 保持 shader-visible slot 与 handle 的复用节奏一致。
        self.gpu_store.bindless_manager.begin_frame(frame_token);
        self.material_bridge.begin_frame(frame_token);
        self.instance_bridge.begin_frame(frame_token);

        self.dispatch_loaded_asset_events();
    }

    /// 执行内部 frame-settings 同步并获取 swapchain image，
    /// 然后返回供外部 CPU 端更新使用的上下文。
    ///
    /// `acquire_image` 放在 update 前，保证本帧的 swapchain image、frame extent 和后续
    /// render graph 导入的 present target 指向同一个窗口状态。
    pub fn update_phase(&mut self) -> RenderRuntimeUpdateCtx<'_> {
        let _span = tracy_client::span!("RenderRuntime::update_phase");

        self.update_frame_settings();
        self.acquire_image();

        RenderRuntimeUpdateCtx {
            world: &mut self.world,
            pipeline_settings: &mut self.gpu_store.pipeline_settings,
            frame_settings: &self.gpu_store.frame_settings,
            accum_data: &self.gpu_store.accum_data,
            swapchain_extent: self.gpu_store.frame_settings.frame_extent,
            delta_time_s: self.gpu_store.delta_time_s,
        }
    }

    /// 更新累积帧跟踪，并上传 GPU scene/descriptor 数据。
    ///
    /// 这是 update 与 render 之间的语义翻译边界：App 仍拥有 camera/input state，
    /// runtime 只读取 camera 快照，并把 `World`、asset/material/instance bridge 的状态整理成
    /// render pass 可读取的 `RenderSceneView`。
    pub fn prepare(&mut self, camera: &Camera) {
        let _span = tracy_client::span!("RenderRuntime::prepare");

        self.update_accum_data(camera);
        self.prepare_gpu_scene(camera);
        self.update_perframe_descriptor_set();
    }

    /// prepare 后、render graph 组图前的 App 同步查询阶段。
    ///
    /// 此阶段只暴露同步 raycast 能力。GPU scene/TLAS 已提交到 graphics queue，后续
    /// raycast 提交会通过同队列顺序看到 prepare 结果，并用自身 fence 阻塞读回。
    pub fn ray_cast_phase(&mut self) -> RenderRuntimeRayCastCtx<'_> {
        RenderRuntimeRayCastCtx {
            device_ctx: self.gfx.device_ctx(),
            resource_ctx: self.gfx.resource_ctx(),
            queue_ctx: self.gfx.queue_ctx(),
            gpu_store: &self.gpu_store,
            render_scene: &self.gpu_scene,
            instance_bridge: &self.instance_bridge,
            ray_cast_service: &mut self.ray_cast_service,
        }
    }

    /// 共享借用：render 阶段中 RenderRuntime 状态只读。
    ///
    /// 这个 Ctx 面向 RenderGraph/pass 录制。它故意不暴露 `World` 的可变借用，避免 render
    /// 阶段继续改变 CPU scene，破坏 `prepare` 已经生成的 GPU scene 快照。
    pub fn render_phase(&self) -> RenderRuntimeRenderCtx<'_> {
        RenderRuntimeRenderCtx {
            device_ctx: self.gfx.device_ctx(),
            resource_ctx: self.gfx.resource_ctx(),
            queue_ctx: self.gfx.queue_ctx(),
            device_info_ctx: self.gfx.device_info_ctx(),
            gpu_store: &self.gpu_store,
            render_scene: &self.gpu_scene,
            render_present: self.render_present.as_ref().unwrap().view(),
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
        let _span = tracy_client::span!("RenderRuntime::end_frame");
        self.gpu_store.frame_counter.next_frame();
    }

    /// 查询是否已经到达下一帧的渲染时间。
    ///
    /// 该方法只做时间判断，不推进 frame counter，也不会等待 GPU。
    pub fn time_to_render(&self) -> bool {
        self.gpu_store.frame_counter.frame_delta_time_limit_us() < self.timer.elapsed_since_tick().as_micros() as f32
    }

    /// 处理窗口 resize。只有 present 层实际重建 swapchain 时才返回 `Some(ctx)`。
    ///
    /// 上层应只在返回上下文时重建与窗口尺寸绑定的 pipeline/render target 资源。
    /// 连续窗口事件会先在 `RenderPresent` 中合并为 latest-size 标记，避免每个事件都触发重建。
    pub fn handle_resize(&mut self, new_size: [u32; 2]) -> Option<RenderRuntimeResizeCtx<'_>> {
        let render_present = self.render_present.as_mut().unwrap();
        render_present.update_window_size(new_size);

        if !render_present.need_resize(self.gfx.surface_ctx()) {
            return None;
        }

        render_present.rebuild_after_resized(
            self.gfx.resource_ctx(),
            self.gfx.device_ctx(),
            self.gfx.surface_ctx(),
            &mut self.gpu_store.gfx_resource_manager,
        );

        Some(RenderRuntimeResizeCtx {
            device_ctx: self.gfx.device_ctx(),
            resource_ctx: self.gfx.resource_ctx(),
            immediate_ctx: self.gfx.immediate_ctx(),
            surface_ctx: self.gfx.surface_ctx(),
            gpu_store: &mut self.gpu_store,
            render_present: self.render_present.as_ref().unwrap().view(),
        })
    }

    /// 生成 shutdown 阶段上下文，供 app/plugin 在 runtime 子资源销毁前释放自己持有的 GPU 资源。
    ///
    /// 这个阶段仍暴露 `GpuStore` 与 `CmdAllocator` 的可变借用，但不再允许继续进入 update/render
    /// 帧流程；调用者应在 `wait_idle` 后使用它清理长期资源，再让 `destroy` 接管 runtime-owned 资源。
    pub fn shutdown_phase(&mut self) -> RenderRuntimeShutdownCtx<'_> {
        RenderRuntimeShutdownCtx {
            device_ctx: self.gfx.device_ctx(),
            resource_ctx: self.gfx.resource_ctx(),
            queue_ctx: self.gfx.queue_ctx(),
            immediate_ctx: self.gfx.immediate_ctx(),
            surface_ctx: self.gfx.surface_ctx(),
            gpu_store: &mut self.gpu_store,
            cmd_allocator: &mut self.cmd_allocator,
        }
    }

    /// window/surface 创建后的一次性初始化。返回用于 plugin 初始化的上下文。
    ///
    /// `RenderRuntime::new` 不触碰窗口系统对象；surface/swapchain 必须等平台层提供 raw handle 后
    /// 才能创建。这样可以保持 runtime 初始化和窗口生命周期之间的清晰边界。
    pub fn init_after_window(
        &mut self,
        raw_display_handle: RawDisplayHandle,
        raw_window_handle: RawWindowHandle,
        window_physical_size: [u32; 2],
    ) -> RenderRuntimeInitCtx<'_> {
        self.render_present = Some(RenderPresent::new(
            self.gfx.resource_ctx(),
            self.gfx.device_ctx(),
            self.gfx.surface_ctx(),
            &mut self.gpu_store.gfx_resource_manager,
            raw_display_handle,
            raw_window_handle,
            vk::Extent2D {
                width: window_physical_size[0],
                height: window_physical_size[1],
            },
        ));

        RenderRuntimeInitCtx {
            device_ctx: self.gfx.device_ctx(),
            resource_ctx: self.gfx.resource_ctx(),
            queue_ctx: self.gfx.queue_ctx(),
            device_info_ctx: self.gfx.device_info_ctx(),
            immediate_ctx: self.gfx.immediate_ctx(),
            surface_ctx: self.gfx.surface_ctx(),
            world: &mut self.world,
            gpu_store: &mut self.gpu_store,
            cmd_allocator: &mut self.cmd_allocator,
            swapchain_image_info: self.render_present.as_ref().unwrap().swapchain_image_info(),
            render_present: self.render_present.as_ref().unwrap().view(),
        }
    }
}

// ---------------------------------------------------------------------------
// 资产事件与 prepare 数据上传
// ---------------------------------------------------------------------------
impl RenderRuntime {
    /// 消费 `AssetHub::update` 产出的加载事件，并转发给对应 render-side owner。
    ///
    /// texture 与 mesh 事件会进入 GPU 上传队列；material 事件会进入稳定 slot 映射。
    /// model ready/failed 状态由 app 通过 `AssetHub` 查询，不通过 runtime 事件分发。
    fn dispatch_loaded_asset_events(&mut self) {
        let _span = tracy_client::span!("RenderRuntime::dispatch_loaded_asset_events");
        let loaded_asset_events = self.world.asset_hub.update();
        let mut texture_events = Vec::new();
        let mut mesh_events = Vec::new();
        let mut material_events = Vec::new();
        for event in loaded_asset_events {
            // 事件分流集中在 runtime 的帧开始阶段，避免各 asset manager 直接接触完整
            // asset event 集合，也让它们可以用更窄的事件集合维护自身契约。
            match event {
                event @ (AssetLoadedEvent::TextureLoaded { .. } | AssetLoadedEvent::TextureFailed { .. }) => {
                    texture_events.push(event);
                }
                event @ AssetLoadedEvent::MeshLoaded { .. } => {
                    mesh_events.push(event);
                }
                event @ AssetLoadedEvent::MaterialLoaded { .. } => {
                    material_events.push(event);
                }
            }
        }

        self.asset_texture_manager.update(
            texture_events,
            self.gfx.resource_ctx(),
            self.gfx.device_ctx(),
            self.gfx.queue_ctx(),
            &mut self.gpu_store.gfx_resource_manager,
            &mut self.gpu_store.bindless_manager,
        );
        self.asset_mesh_manager.update(
            mesh_events,
            self.gfx.resource_ctx(),
            self.gfx.device_ctx(),
            self.gfx.queue_ctx(),
        );
        self.material_bridge.apply_material_events(material_events);
    }

    /// 根据 app camera 快照更新累积帧计数。
    ///
    /// 累积渲染只关心相机方向和位置是否变化；后续 pass 根据这里的计数决定是否复用上一帧结果。
    fn update_accum_data(&mut self, camera: &Camera) {
        let current_camera_dir = glam::vec3(camera.euler_yaw_deg, camera.euler_pitch_deg, camera.euler_roll_deg);
        self.gpu_store.accum_data.update_accum_frames(current_camera_dir, camera.position);
    }

    /// 合成 `GpuScene` 用于判断 TLAS 是否过期的 scene revision。
    ///
    /// mesh ready revision 覆盖 BLAS 新增/替换，instance revision 覆盖实例增删、ready 状态
    /// 和 transform 变化；使用 saturating add 保证长时间运行时不会回绕成旧 revision。
    fn combine_scene_revision(mesh_ready_revision: u64, instance_revision: u64) -> u64 {
        mesh_ready_revision.saturating_add(instance_revision)
    }

    /// 准备 render pass 可见的 GPU scene 与 per-frame uniform。
    ///
    /// 该函数把所有 staging copy 录到同一个 command buffer，最后一次提交到 graphics queue；
    /// render graph 在后续命令提交中通过常规 queue 顺序看到这些写入。
    fn prepare_gpu_scene(&mut self, camera: &Camera) {
        let _span = tracy_client::span!("RenderRuntime::prepare_gpu_scene");
        let frame_extent = self.gpu_store.frame_settings.frame_extent;
        let frame_label = self.gpu_store.frame_counter.frame_label();
        let cmd = self.gpu_scene_update_cmds[*frame_label].clone();

        // GPU scene 更新使用独立命令缓冲，把 material/instance/geometry/light/scene buffer
        // 的 staging copy 和 barrier 串在一起，作为 render graph 录制前的固定准备阶段。
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

        let bindless_target = self.gpu_store.global_descriptor_sets.bindless_target();
        // bindless 表先更新，因为 material upload 可能立即解析 texture SRV handle；
        // 后续 scene root buffer 也会写入默认环境贴图的 bindless handle。
        self.gpu_store.bindless_manager.prepare_render_data(
            self.gfx.device_ctx(),
            &self.gpu_store.gfx_resource_manager,
            bindless_target,
        );

        // material loaded 事件已在 begin_frame 进入稳定 slot；这里只根据 texture ready/fallback
        // 状态写当前 FIF 的 material buffer。
        self.material_bridge.update_textures(&self.asset_texture_manager);
        self.material_bridge.upload(
            self.gfx.resource_ctx(),
            &cmd,
            transfer_barrier_mask,
            frame_label,
            &self.asset_texture_manager,
        );

        // instance 阶段是 CPU scene 到 render-side `RenderData` 的边界；只有 mesh 与 material
        // 都解析成功的实例会进入 active 列表。
        let scene_render_data = self.instance_bridge.prepare_render_data(
            &self.world.scene_manager,
            &self.material_bridge,
            &self.asset_mesh_manager,
        );
        let material_buffer_device_address = self.material_bridge.material_buffer_device_address(frame_label);
        // mesh ready 与 instance 变化都会影响 TLAS；两个 revision 合成一条 scene revision，
        // 交给 GpuScene 判断当前 FIF 的 TLAS 是否需要重建。
        let scene_revision =
            Self::combine_scene_revision(self.asset_mesh_manager.ready_revision(), self.instance_bridge.revision());
        self.gpu_scene.upload_render_data(
            self.gfx.resource_ctx(),
            self.gfx.device_ctx(),
            self.gfx.immediate_ctx(),
            &cmd,
            transfer_barrier_mask,
            &self.gpu_store.frame_counter,
            &scene_render_data,
            material_buffer_device_address,
            scene_revision,
            &self.gpu_store.bindless_manager,
        );

        let view = camera.get_view_matrix();
        let projection = camera.get_projection_matrix();
        // per-frame uniform 放在 GPU scene 上传之后写入同一条命令缓冲，保证本帧 shader
        // 看到的相机、分辨率、时间和 scene buffer 都来自同一个 prepare 快照。
        let per_frame_data = gpu::PerFrameData {
            projection: projection.into(),
            view: view.into(),
            inv_view: view.inverse().into(),
            inv_projection: projection.inverse().into(),
            camera_pos: camera.position.into(),
            camera_forward: camera.camera_forward().into(),
            time_ms: self.timer.total_time_ms(),
            delta_time_ms: self.timer.delta_time_ms(),
            frame_id: self.gpu_store.frame_counter.frame_id(),
            resolution: gpu::Float2 {
                x: frame_extent.width as f32,
                y: frame_extent.height as f32,
            },
            accum_frames: self.gpu_store.accum_data.accum_frames_num() as u32,
            _padding_0: Default::default(),
            _padding_1: Default::default(),
            _padding_2: Default::default(),
        };
        let crt_frame_data_buffer = &self.gpu_store.per_frame_data_buffers[*frame_label];
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
}

// ---------------------------------------------------------------------------
// 内部辅助函数
// ---------------------------------------------------------------------------
impl RenderRuntime {
    /// 为当前 FIF frame label acquire swapchain image。
    ///
    /// 该 helper 只在 update 阶段调用；成功后 present view 的 current image 与本帧
    /// render graph 导入的 target 保持一致。
    fn acquire_image(&mut self) {
        self.render_present
            .as_mut()
            .unwrap()
            .acquire_image(self.gfx.surface_ctx(), self.gpu_store.frame_counter.frame_label());
    }

    /// 同步 swapchain extent 到 `FrameSettings`，并在尺寸变化时重建 FIF framebuffer 资源。
    ///
    /// present 层负责判断 swapchain 是否需要重建；这里处理的是 runtime 内部与 frame extent
    /// 绑定的渲染资源。
    fn update_frame_settings(&mut self) {
        let swapchain_extent = self.render_present.as_ref().unwrap().extent();
        if self.gpu_store.frame_settings.frame_extent == swapchain_extent {
            return;
        }

        if self.gpu_store.frame_settings.frame_extent != swapchain_extent {
            // frame extent 变化会让历史累积结果失效，并要求所有 per-FIF render target
            // 以新尺寸重建。
            self.gpu_store.frame_settings.frame_extent = swapchain_extent;
            self.resize_frame_buffer(swapchain_extent);
        }
    }

    /// 重建所有依赖窗口尺寸的 runtime-owned frame buffer。
    ///
    /// resize 路径使用 device idle 作为保守同步点，避免旧 extent 的 render target 仍被
    /// 在飞命令引用时被释放或复用。
    fn resize_frame_buffer(&mut self, new_extent: vk::Extent2D) {
        self.gpu_store.accum_data.reset();

        self.gfx.device_ctx().device().wait_idle();
        self.gpu_store.frame_settings.frame_extent = new_extent;

        self.gpu_store.fif_buffers.rebuild(
            self.gfx.resource_ctx(),
            self.gfx.device_ctx(),
            self.gfx.immediate_ctx(),
            &mut self.gpu_store.bindless_manager,
            &mut self.gpu_store.gfx_resource_manager,
            &self.gpu_store.frame_settings,
            &self.gpu_store.frame_counter,
        );
    }

    /// 刷新当前 FIF per-frame descriptor set。
    ///
    /// descriptor 指向刚写入的 per-frame UBO 和 `GpuScene` scene root buffer；render pass
    /// 通过全局 descriptor set 读取本帧相机、时间与 scene device address。
    fn update_perframe_descriptor_set(&mut self) {
        let frame_label = self.gpu_store.frame_counter.frame_label();
        let per_frame_data_buffer = &self.gpu_store.per_frame_data_buffers[*frame_label];
        let gpu_scene_buffer = self.gpu_scene.scene_buffer(frame_label);
        let perframe_set = self.gpu_store.global_descriptor_sets.current_perframe_set(frame_label).handle();

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
