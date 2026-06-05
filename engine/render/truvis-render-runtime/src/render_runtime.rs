use std::{env, ffi::CStr};

use ash::vk::{self, Handle};
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
use truvis_render_foundation::dlss_sr::{DlssSrMode, DlssSrState};
use truvis_render_foundation::frame_counter::FrameCounter;
use truvis_render_foundation::frame_state::FrameRenderState;
use truvis_render_foundation::gfx_resource_manager::GfxResourceManager;
use truvis_render_foundation::global_descriptor_sets::{GlobalDescriptorSets, PerFrameDescriptorBinding};
use truvis_render_foundation::render_options::RenderOptions;
use truvis_render_foundation::render_view::RenderView;
use truvis_render_foundation::sampler_manager::RenderSamplerManager;
use truvis_render_foundation::view_accum::ViewAccumState;
use truvis_shader_binding::gpu;
use truvis_world::scene_manager::SceneManager;

use truvis_render_foundation::gpu_store::GpuStore;
use truvis_streamline_binding::dlss;
use truvis_world::World;

use crate::asset_mesh_manager::AssetMeshManager;
use crate::asset_texture_manager::AssetTextureManager;
use crate::environment_binding::EnvironmentBinding;
use crate::frame_timer::FrameTimer;
use crate::instance_bridge::InstanceBridge;
use crate::material_bridge::MaterialBridge;
use crate::present::swapchain_presenter::SwapchainPresenter;
use crate::ray_cast::RayCastService;
use crate::render_scene::gpu_scene::GpuScene;
use crate::runtime_defaults::DefaultRenderRuntimeSettings;
use crate::sky_bridge::SkyBridge;

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
/// render_runtime.prepare(render_view);
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
    sky_bridge: SkyBridge,
    asset_mesh_manager: AssetMeshManager,
    material_bridge: MaterialBridge,
    instance_bridge: InstanceBridge,
    ray_cast_service: RayCastService,

    cmd_allocator: CmdAllocator,

    timer: FrameTimer,
    fif_timeline_semaphore: GfxSemaphore,

    gpu_scene_update_cmds: Vec<GfxCommandBuffer>,

    swapchain_presenter: Option<SwapchainPresenter>,
    last_applied_dlss_sr_mode: DlssSrMode,
}

// 创建与初始化
impl RenderRuntime {
    /// 创建不依赖窗口系统的 runtime root state。
    ///
    /// 这里会初始化 `Gfx`、CPU `World`、GPU `GpuStore`、资产管理器、SkyBridge、
    /// material/instance bridge、私有 `GpuScene` 和全局描述符，但不会创建 surface/swapchain。窗口相关资源必须等
    /// `init_after_window` 收到平台层 raw handle 后再创建。
    pub fn new(extra_instance_ext: Vec<&'static CStr>) -> Self {
        let _span = tracy_client::span!("RenderRuntime::new");

        let gfx = {
            let _span = tracy_client::span!("RenderRuntime::new/Gfx");
            Gfx::new("Truvis".to_string(), extra_instance_ext)
        };
        Self::query_streamline_dlss_support(&gfx);

        let frame_state = {
            let _span = tracy_client::span!("RenderRuntime::new/frame_state");
            // runtime 创建时还没有 surface/swapchain，只能先保存格式和一个占位 extent。
            // 真实窗口尺寸会在 `init_after_window` 创建 present 后同步，并交给 app/plugin
            // 初始化自己的 window-sized render targets。
            FrameRenderState {
                hdr_color_format: vk::Format::R32G32B32A32_SFLOAT,
                depth_format: Self::get_depth_format(gfx.device_info_ctx()),
                render_extent: vk::Extent2D {
                    width: 400,
                    height: 400,
                },
                output_extent: vk::Extent2D {
                    width: 400,
                    height: 400,
                },
            }
        };

        let (timer, view_accum, fif_timeline_semaphore) = {
            let _span = tracy_client::span!("RenderRuntime::new/sync");
            (
                FrameTimer::default(),
                ViewAccumState::default(),
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
        let mut asset_hub = {
            let _span = tracy_client::span!("RenderRuntime::new/asset_hub");
            AssetHub::new()
        };
        let sky_bridge = {
            let _span = tracy_client::span!("RenderRuntime::new/sky_bridge");
            SkyBridge::new(
                gfx.resource_ctx(),
                gfx.device_ctx(),
                gfx.immediate_ctx(),
                &mut asset_hub,
                &mut gfx_resource_manager,
                &mut bindless_manager,
            )
        };
        let gpu_scene = {
            let _span = tracy_client::span!("RenderRuntime::new/gpu_scene");
            GpuScene::new(gfx.resource_ctx())
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
                swapchain_presenter: None,
                last_applied_dlss_sr_mode: DlssSrMode::Off,

                world: World {
                    scene_manager,
                    asset_hub,
                },
                asset_texture_manager,
                sky_bridge,
                asset_mesh_manager,
                material_bridge,
                instance_bridge,
                ray_cast_service,
                gpu_scene,
                gpu_store: GpuStore {
                    bindless_manager,
                    global_descriptor_sets: render_descriptor_sets,
                    gfx_resource_manager,
                    sampler_manager,
                    per_frame_data_buffers,

                    frame_counter,
                    frame_state,
                    render_options: Self::initial_render_options(),
                    dlss_sr_state: DlssSrState::default(),

                    delta_time_s: 0.0,
                    total_time_s: 0.0,
                    view_accum,
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

    fn query_streamline_dlss_support(gfx: &Gfx) {
        // 当前 Gfx 从 sl.interposer.dll 加载 Vulkan entry，Streamline 会通过
        // vkCreateInstance/vkCreateDevice proxy 绑定 Vulkan root；手动调用
        // slSetVulkanInfo 只适用于不走 proxy 的集成方式。
        match dlss::query_support(gfx.physical_device().vk_handle().as_raw()) {
            Ok(support) => {
                log::info!(
                    "DLSS SR support: supported={}, flags={}, max_viewports={}, max_cpu_threads={}",
                    support.supported,
                    support.flags,
                    support.max_num_viewports,
                    support.max_num_cpu_threads
                );
            }
            Err(err) => {
                log::warn!("DLSS SR support query failed: {}", err);
            }
        }
    }

    fn initial_render_options() -> RenderOptions {
        let mut options = RenderOptions::default();

        // 环境变量只作为启动时调试入口，便于自动化 validation/resize 测试直接进入指定 SR mode。
        // 运行中 mode 仍由 ImGui 修改 `RenderOptions`，再由 sync_render_options_frame_state 统一生效。
        if let Ok(value) = env::var("TRUVIS_DLSS_SR_MODE") {
            match DlssSrMode::from_config_value(&value) {
                Some(mode) => {
                    options.dlss_sr_mode = mode;
                    log::info!("Initial DLSS SR mode from TRUVIS_DLSS_SR_MODE={value}: {mode:?}");
                }
                None => {
                    log::warn!("Ignoring unsupported TRUVIS_DLSS_SR_MODE value: {value}");
                }
            }
        }

        options
    }
}

fn to_streamline_dlss_mode(mode: DlssSrMode) -> dlss::DlssMode {
    match mode {
        DlssSrMode::Off => dlss::DlssMode::Off,
        DlssSrMode::Dlaa => dlss::DlssMode::Dlaa,
        DlssSrMode::Quality => dlss::DlssMode::Quality,
        DlssSrMode::Balanced => dlss::DlssMode::Balanced,
        DlssSrMode::Performance => dlss::DlssMode::Performance,
        DlssSrMode::UltraPerformance => dlss::DlssMode::UltraPerformance,
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
    /// 先释放 present/asset/GpuScene/command/descriptor 等对象，最后销毁 `Gfx`，
    /// 这样所有 Vulkan wrapper 都能通过有效的 typed Ctx 显式释放。
    pub fn destroy(mut self) {
        self.gfx.wait_idel();

        // present 持有 surface/swapchain 与 WSI image wrapper，必须先释放；后续 scene
        // 资源销毁不再需要访问当前窗口 target。
        if let Some(swapchain_presenter) = self.swapchain_presenter.take() {
            swapchain_presenter.destroy(
                self.gfx.resource_ctx(),
                self.gfx.device_ctx(),
                self.gfx.surface_ctx(),
                &mut self.gpu_store.gfx_resource_manager,
            );
        }

        self.ray_cast_service.destroy_mut(self.gfx.resource_ctx(), self.gfx.device_ctx());
        // CPU scene/asset 与 render-side bridge 按依赖方向释放：先停止 scene runtime，
        // 再释放 material/texture/mesh/GpuScene 等 GPU 翻译缓存。
        self.world.scene_manager.destroy();
        self.material_bridge.destroy(self.gfx.resource_ctx());
        self.sky_bridge.destroy_mut(
            self.gfx.resource_ctx(),
            self.gfx.device_ctx(),
            &mut self.gpu_store.bindless_manager,
            &mut self.gpu_store.gfx_resource_manager,
        );
        self.asset_texture_manager.destroy(
            self.gfx.resource_ctx(),
            self.gfx.device_ctx(),
            &mut self.gpu_store.gfx_resource_manager,
            &mut self.gpu_store.bindless_manager,
        );
        self.world.asset_hub.destroy();
        self.gpu_scene.destroy_mut(self.gfx.resource_ctx(), self.gfx.device_ctx());
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

    /// 执行内部 frame state 同步并获取 swapchain image，
    /// 然后返回供外部 CPU 端更新使用的上下文。
    ///
    /// `acquire_image` 放在 update 前，保证本帧的 swapchain image、frame state 和后续
    /// render graph 导入的 present target 指向同一个窗口状态。
    pub fn update_phase(&mut self) -> RenderRuntimeUpdateCtx<'_> {
        let _span = tracy_client::span!("RenderRuntime::update_phase");

        self.update_frame_state();
        self.acquire_image();

        RenderRuntimeUpdateCtx {
            world: &mut self.world,
            render_options: &mut self.gpu_store.render_options,
            frame_state: &self.gpu_store.frame_state,
            view_accum: &self.gpu_store.view_accum,
            swapchain_extent: self.gpu_store.frame_state.output_extent,
            delta_time_s: self.gpu_store.delta_time_s,
        }
    }

    /// 更新累积帧跟踪，并上传 GPU scene/descriptor 数据。
    ///
    /// 这是 update 与 render 之间的语义翻译边界：App 仍拥有 camera/input state，
    /// runtime 只读取 render view 快照，并把 `World`、asset/material/instance bridge 的状态整理成
    /// render pass 可读取的 `RenderSceneView`。
    pub fn prepare(&mut self, render_view: &RenderView) {
        let _span = tracy_client::span!("RenderRuntime::prepare");

        self.update_view_accum(render_view);
        // DLSS constants 与本帧相机快照绑定，必须在 render graph 录制 evaluate 前更新。
        self.gpu_store.dlss_sr_state.update(render_view, &self.gpu_store.frame_state);
        self.prepare_gpu_scene(render_view);
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
        assert!(
            self.current_frame_has_present_target(),
            "Render phase requested without a successfully acquired present target"
        );
        RenderRuntimeRenderCtx {
            device_ctx: self.gfx.device_ctx(),
            resource_ctx: self.gfx.resource_ctx(),
            queue_ctx: self.gfx.queue_ctx(),
            device_info_ctx: self.gfx.device_info_ctx(),
            gpu_store: &self.gpu_store,
            render_scene: &self.gpu_scene,
            present: self.swapchain_presenter.as_ref().unwrap().view(),
            timeline: &self.fif_timeline_semaphore,
        }
    }

    /// 提交 present 命令。
    ///
    /// 渲染命令提交由上层 render graph 完成；这里仅把当前 swapchain image 交给 present queue，
    /// 并让 present 层记录是否需要在后续帧重建 swapchain。
    pub fn present(&mut self) {
        self.swapchain_presenter.as_mut().unwrap().present_image(self.gfx.surface_ctx(), self.gfx.queue_ctx());
    }

    /// 当前帧是否成功 acquire 到 present target。
    ///
    /// 返回 false 时，WSI 没有把 swapchain image ownership 交给应用侧，也没有 signal
    /// acquire semaphore；上层必须跳过 prepare/render/present。
    #[inline]
    pub fn current_frame_has_present_target(&self) -> bool {
        self.swapchain_presenter.as_ref().unwrap().current_image_acquired()
    }

    /// 根据当前 render options 同步 frame render state。
    ///
    /// DLSS mode 变化可能只影响 pass 分支，也可能改变低分辨率 render extent。前者只需要
    /// 重置 DLSS history，后者必须让 app/plugin 重建 RT/GBuffer/DLSS input targets。
    pub fn sync_render_options_frame_state(&mut self) -> Option<RenderRuntimeResizeCtx<'_>> {
        let old_state = self.gpu_store.frame_state;
        let old_mode = self.last_applied_dlss_sr_mode;
        let requested_mode = self.gpu_store.render_options.dlss_sr_mode;
        let output_extent = self.swapchain_presenter.as_ref().unwrap().extent();
        if old_mode == requested_mode && old_state.output_extent == output_extent {
            return None;
        }

        let new_state = self.resolve_frame_state_for_output(output_extent);
        let new_mode = self.gpu_store.render_options.dlss_sr_mode;
        let mode_changed = old_mode != new_mode;

        if !mode_changed && old_state == new_state {
            return None;
        }

        if mode_changed {
            log::info!("DLSS SR mode changed: {:?} -> {:?}", old_mode, new_mode);
            self.gpu_store.dlss_sr_state.request_reset();
            if new_mode == DlssSrMode::Off {
                // 退出 SR 时释放 viewport 0 的 DLSS 内部资源；其它 app-owned image
                // 由后续 resize ctx 或 plugin shutdown 负责销毁。
                if let Err(err) = dlss::free_resources(0) {
                    log::warn!("Failed to free DLSS SR resources for viewport 0: {}", err);
                }
            }
        }

        self.last_applied_dlss_sr_mode = new_mode;

        if old_state == new_state {
            return None;
        }

        log::info!(
            "Frame render state changed: render={}x{}, output={}x{} -> render={}x{}, output={}x{}",
            old_state.render_extent.width,
            old_state.render_extent.height,
            old_state.output_extent.width,
            old_state.output_extent.height,
            new_state.render_extent.width,
            new_state.render_extent.height,
            new_state.output_extent.width,
            new_state.output_extent.height
        );

        // 这是非 WSI resize 的运行时 target 尺寸变化，旧 per-frame image 可能仍被前几帧引用；
        // 重建前等待 device idle，保持 target owner 的显式 destroy/rebuild 路径简单可靠。
        self.gfx.wait_idel();
        self.gpu_store.frame_state = new_state;
        self.gpu_store.view_accum.reset();
        // render extent 变化会让 DLSS history 的 sample grid 失效，即使相机没有变化也必须 reset。
        self.gpu_store.dlss_sr_state.request_reset();

        Some(RenderRuntimeResizeCtx {
            device_ctx: self.gfx.device_ctx(),
            resource_ctx: self.gfx.resource_ctx(),
            immediate_ctx: self.gfx.immediate_ctx(),
            surface_ctx: self.gfx.surface_ctx(),
            gpu_store: &mut self.gpu_store,
            present: self.swapchain_presenter.as_ref().unwrap().view(),
        })
    }

    /// 是否存在等待处理的 swapchain 重建请求。
    #[inline]
    pub fn has_pending_swapchain_recreate(&self) -> bool {
        self.swapchain_presenter.as_ref().unwrap().has_pending_resize()
    }

    /// 为没有 GPU render graph 的帧补齐 FIF timeline signal。
    ///
    /// resize/out-of-date 期间可能 acquire 不到 swapchain image。此时本帧不会录制
    /// render graph，但 frame counter 仍需要前进；提交一个空 signal 可以保持后续
    /// `begin_frame` 对 timeline 的等待不会落到永远无人 signal 的 frame id 上。
    pub fn signal_current_frame_complete(&self) {
        let frame_id = self.gpu_store.frame_counter.frame_id();
        let submit_info = GfxSubmitInfo::new(&[]).signal(
            &self.fif_timeline_semaphore,
            vk::PipelineStageFlags2::BOTTOM_OF_PIPE,
            Some(frame_id),
        );
        self.gfx.queue_ctx().gfx_queue().submit(vec![submit_info], None);
    }

    /// 推进帧计数器。
    ///
    /// 所有按 `FrameCounter` 轮转的资源都在此之后切到下一帧标签；因此必须放在
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
    /// 连续窗口事件会先在 `SwapchainPresenter` 中合并为 latest-size 标记，避免每个事件都触发重建。
    pub fn handle_resize(&mut self, new_size: [u32; 2]) -> Option<RenderRuntimeResizeCtx<'_>> {
        let swapchain_presenter = self.swapchain_presenter.as_mut().unwrap();
        swapchain_presenter.update_window_size(new_size);

        if !swapchain_presenter.need_resize(self.gfx.surface_ctx()) {
            return None;
        }

        swapchain_presenter.rebuild_after_resized(
            self.gfx.resource_ctx(),
            self.gfx.device_ctx(),
            self.gfx.surface_ctx(),
            &mut self.gpu_store.gfx_resource_manager,
        );
        // runtime 只同步 frame state；具体 RT / main-view / GBuffer target 的重建由随后
        // 返回的 resize ctx 交给 app/plugin 完成，避免 engine 反向持有管线策略资源。
        self.sync_frame_extent_after_present_resize();

        Some(RenderRuntimeResizeCtx {
            device_ctx: self.gfx.device_ctx(),
            resource_ctx: self.gfx.resource_ctx(),
            immediate_ctx: self.gfx.immediate_ctx(),
            surface_ctx: self.gfx.surface_ctx(),
            gpu_store: &mut self.gpu_store,
            present: self.swapchain_presenter.as_ref().unwrap().view(),
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
        self.swapchain_presenter = Some(SwapchainPresenter::new(
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
        // surface 创建后才能知道平台裁剪后的实际 swapchain extent。这里先同步到
        // `GpuStore.frame_state`，让后续 PluginInitCtx 创建 app-owned target 时拿到真实尺寸。
        self.sync_frame_extent_after_present_resize();

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
            swapchain_image_info: self.swapchain_presenter.as_ref().unwrap().swapchain_image_info(),
            present: self.swapchain_presenter.as_ref().unwrap().view(),
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

    /// 根据 app render view 快照更新 main view 累积帧计数。
    ///
    /// 累积渲染关心最终视图/投影是否变化；后续 pass 根据这里的计数决定是否复用上一帧结果。
    fn update_view_accum(&mut self, render_view: &RenderView) {
        self.gpu_store.view_accum.update_accum_frames(render_view.accum_signature());
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
    fn prepare_gpu_scene(&mut self, render_view: &RenderView) {
        let _span = tracy_client::span!("RenderRuntime::prepare_gpu_scene");
        let frame_extent = self.gpu_store.frame_state.render_extent;
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
        // bindless 表先更新，因为 material upload 和环境绑定都可能立即解析 texture SRV handle；
        // 后续 scene root buffer 会写入这些 shader-visible handle。
        self.gpu_store.bindless_manager.prepare_render_data(
            self.gfx.device_ctx(),
            &self.gpu_store.gfx_resource_manager,
            bindless_target,
        );
        let sky_update = self.sky_bridge.update_sky_binding(&self.asset_texture_manager);
        if sky_update.changed {
            // sky 从 fallback 切换到真实贴图时，历史累积帧已经不再对应当前环境光。
            self.gpu_store.view_accum.reset();
        }
        let environment_binding = EnvironmentBinding {
            sky: sky_update.binding,
        };

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
            environment_binding,
        );

        // per-frame uniform 放在 GPU scene 上传之后写入同一条命令缓冲，保证本帧 shader
        // 看到的相机、分辨率、时间和 scene buffer 都来自同一个 prepare 快照。
        let per_frame_data = gpu::PerFrameData {
            projection: render_view.projection.into(),
            view: render_view.view.into(),
            inv_view: render_view.inv_view.into(),
            inv_projection: render_view.inv_projection.into(),
            camera_pos: render_view.position_ws.into(),
            camera_forward: render_view.forward_ws.into(),
            time_ms: self.timer.total_time_ms(),
            delta_time_ms: self.timer.delta_time_ms(),
            frame_id: self.gpu_store.frame_counter.frame_id(),
            resolution: gpu::Float2 {
                x: frame_extent.width as f32,
                y: frame_extent.height as f32,
            },
            // 主流程已不再做 progressive accumulation；保持为 0 可以让 raygen 每帧稳定写入当前图像。
            accum_frames: 0,
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
    fn acquire_image(&mut self) -> bool {
        self.swapchain_presenter
            .as_mut()
            .unwrap()
            .acquire_image(self.gfx.surface_ctx(), self.gpu_store.frame_counter.frame_label())
    }

    /// 同步 swapchain extent 到 `FrameRenderState`。
    ///
    /// present 层负责判断 swapchain 是否需要重建；具体窗口尺寸 render target
    /// 属于 app/plugin owner，这里只维护 runtime 的 frame state。
    fn update_frame_state(&mut self) {
        self.sync_frame_extent_after_present_resize();
    }

    /// 同步 present extent 到 runtime frame state，并在尺寸变化时清空历史累积。
    fn sync_frame_extent_after_present_resize(&mut self) {
        let swapchain_extent = self.swapchain_presenter.as_ref().unwrap().extent();
        if self.gpu_store.frame_state.output_extent == swapchain_extent {
            return;
        }

        // 尺寸变化会让历史累积图像的内容语义失效，但图像本身属于 app-owned target。
        // runtime 只更新 shader/per-frame data 会读取的 extent，并清零累积帧计数；
        // 具体 image 重建在 Plugin::on_resize 中发生。
        let old_mode = self.last_applied_dlss_sr_mode;
        self.gpu_store.frame_state = self.resolve_frame_state_for_output(swapchain_extent);
        let new_mode = self.gpu_store.render_options.dlss_sr_mode;
        if old_mode != new_mode && new_mode == DlssSrMode::Off {
            // resize 期间查询 optimal settings 失败可能把 SR mode 降级为 Off；此时同样需要释放
            // 原 viewport resource，避免后续 native 路径还保留旧 DLSS state。
            if let Err(err) = dlss::free_resources(0) {
                log::warn!("Failed to free DLSS SR resources for viewport 0 after resize fallback: {}", err);
            }
        }
        self.last_applied_dlss_sr_mode = new_mode;
        self.gpu_store.view_accum.reset();
        self.gpu_store.dlss_sr_state.request_reset();
    }

    fn resolve_frame_state_for_output(&mut self, output_extent: vk::Extent2D) -> FrameRenderState {
        let mut frame_state = self.gpu_store.frame_state;
        frame_state.output_extent = output_extent;

        let mode = self.gpu_store.render_options.dlss_sr_mode;
        if mode == DlssSrMode::Off || mode == DlssSrMode::Dlaa {
            // Off 是 native fallback；DLAA 仍走 kFeatureDLSS，但不做低分辨率渲染。
            frame_state.render_extent = output_extent;
            return frame_state;
        }

        // SR upscale mode 由 Streamline 决定低分辨率 render extent；app-owned RT/GBuffer/DLSS
        // input targets 会用这个尺寸重建，output 仍保持 swapchain extent。
        let options = dlss::DlssOptions {
            mode: to_streamline_dlss_mode(mode),
            output_width: output_extent.width,
            output_height: output_extent.height,
            color_buffers_hdr: true,
        };
        match dlss::get_optimal_settings(options) {
            Ok(settings) if settings.optimal_render_width > 0 && settings.optimal_render_height > 0 => {
                frame_state.render_extent = vk::Extent2D {
                    width: settings.optimal_render_width,
                    height: settings.optimal_render_height,
                };
                log::info!(
                    "DLSS SR optimal settings: mode={:?}, output={}x{}, render={}x{}, sharpness={:.3}, min={}x{}, max={}x{}",
                    mode,
                    output_extent.width,
                    output_extent.height,
                    settings.optimal_render_width,
                    settings.optimal_render_height,
                    settings.optimal_sharpness,
                    settings.render_width_min,
                    settings.render_height_min,
                    settings.render_width_max,
                    settings.render_height_max
                );
            }
            Ok(settings) => {
                log::warn!(
                    "DLSS SR returned invalid optimal render extent {}x{} for mode {:?}; falling back to Off/native.",
                    settings.optimal_render_width,
                    settings.optimal_render_height,
                    mode
                );
                // 不接受 0 尺寸 optimal settings。直接降级 Off，保证后续 graph 仍有 native target。
                self.gpu_store.render_options.dlss_sr_mode = DlssSrMode::Off;
                frame_state.render_extent = output_extent;
            }
            Err(err) => {
                log::warn!("DLSS SR optimal settings failed for mode {:?}: {}; falling back to Off/native.", mode, err);
                // capability/driver/runtime 异常都按 native fallback 处理，避免因为 SR 不可用阻塞 app 启动。
                self.gpu_store.render_options.dlss_sr_mode = DlssSrMode::Off;
                frame_state.render_extent = output_extent;
            }
        }

        frame_state
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
