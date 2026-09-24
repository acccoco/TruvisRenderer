use std::{ffi::CStr, num::NonZeroU32, time::Duration};

use ash::vk;
use raw_window_handle::{RawDisplayHandle, RawWindowHandle};

use truvis_gfx::commands::barrier::{GfxBarrierMask, GfxBufferBarrier};
use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_gfx::commands::semaphore::GfxSemaphore;
use truvis_gfx::commands::submit_info::GfxSubmitInfo;
use truvis_gfx::gfx::{Gfx, GfxDeviceInfoCtx};
use truvis_gfx::utilities::descriptor_cursor::GfxDescriptorCursor;
use truvis_path::TruvisPath;
use truvis_render_foundation::frame_label::FrameLabel;
use truvis_shader_binding::gpu;
use truvis_world::GameWorld;

use crate::bindings::global_descriptor_sets::PerFrameDescriptorBinding;
use crate::bindings::per_frame_gpu_data::PerFrameGpuData;
use crate::bindings::shader_binding_system::ShaderBindingSystem;
use crate::present::swapchain_presenter::SwapchainPresenter;
use crate::ray_cast::RayCastService;
use crate::render_runtime_ctx::RenderPassRecordCtx;
use crate::render_world::render_world::RenderWorld;
use crate::resources::cmd_allocator::CmdAllocator;
use crate::resources::gfx_resource_registry::GfxResourceRegistry;
use crate::runtime_defaults::DefaultRenderRuntimeSettings;
use crate::state::frame_state::FrameRenderState;
use crate::state::frame_timing::FrameTiming;

pub use crate::render_runtime_ctx::{
    RenderFrameInput, RenderRuntimeInitCtx, RenderRuntimeRayCastCtx, RenderRuntimeRenderCtx, RenderRuntimeResizeCtx,
    RenderRuntimeShutdownCtx, RenderRuntimeUpdateCtx,
};

/// 渲染运行时核心。
///
/// 只通过返回类型化 Ctx 结构的生命周期方法暴露状态。
/// 生命周期由外部代码驱动；RenderRuntime 不感知 GUI、子系统或 Renderer 内部编排概念。
///
/// 它位于 `RenderLoop` 之下、`truvis-gfx` 与 foundation GPU owner 之上，是 CPU scene、
/// render-side 资产上传、GPU scene 翻译、swapchain/present 和 FIF 同步的聚合 owner。
/// 上层只能在对应阶段拿到窄化后的 Ctx，不能长期保存完整 `Gfx` 或 runtime 内部字段。
/// 这保证资源销毁顺序仍由 runtime 集中控制：Renderer/子系统可以在生命周期阶段创建或释放资源，
/// 但不能越过 Ctx 长期持有内部 owner。
///
/// # 生命周期调用顺序
/// ```ignore
/// render_runtime.begin_frame();
/// let update_ctx = render_runtime.update_phase();
/// // ... 使用 update_ctx 执行 Renderer/子系统 CPU 更新 ...
/// drop(update_ctx);
/// render_runtime.prepare(&frame_input);
/// let render_ctx = render_runtime.render_phase();
/// // ... 执行 Renderer/子系统 render graph 工作 ...
/// drop(render_ctx);
/// render_runtime.present();
/// render_runtime.end_frame();
/// ```
pub struct RenderRuntime {
    gfx: Gfx,

    world: GameWorld,
    gfx_resource_registry: GfxResourceRegistry,
    shader_binding_system: ShaderBindingSystem,
    frame_timing: FrameTiming,
    per_frame_gpu_data: PerFrameGpuData,
    frame_state: FrameRenderState,
    requested_render_extent: vk::Extent2D,
    history_invalidated: bool,
    render_world: RenderWorld,
    ray_cast_service: RayCastService,

    cmd_allocator: CmdAllocator,

    fif_timeline_semaphore: GfxSemaphore,

    render_world_update_cmds: Vec<GfxCommandBuffer>,

    swapchain_presenter: Option<SwapchainPresenter>,
}

// 创建与初始化
impl RenderRuntime {
    /// 创建不依赖窗口系统的 runtime root state。
    ///
    /// 这里会初始化 `Gfx`、CPU `GameWorld`、runtime 级 GPU resource/binding/timing owner 和私有
    /// `RenderWorld`；`RenderWorld` 同时持有其内部的 `RenderAssetSystem`。
    /// 这里不会创建 surface/swapchain。窗口相关资源必须等
    /// `init_after_window` 收到平台层 raw handle 后再创建。
    pub fn new(extra_instance_ext: Vec<&'static CStr>) -> Self {
        let _span = tracy_client::span!("RenderRuntime::new");

        let gfx = {
            let _span = tracy_client::span!("RenderRuntime::new/Gfx");
            Gfx::new("Truvis".to_string(), extra_instance_ext)
        };
        let frame_state = {
            let _span = tracy_client::span!("RenderRuntime::new/frame_state");
            // runtime 创建时还没有 surface/swapchain，只能先保存格式和一个占位 extent。
            // 真实窗口尺寸会在 `init_after_window` 创建 present 后同步，并交给 Renderer/子系统
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

        let fif_timeline_semaphore = {
            let _span = tracy_client::span!("RenderRuntime::new/sync");
            GfxSemaphore::new_timeline(gfx.device_ctx(), 0, "render-timeline")
        };

        let (mut gfx_resource_registry, mut cmd_allocator, frame_timing, mut shader_binding_system) = {
            let _span = tracy_client::span!("RenderRuntime::new/managers");
            let gfx_resource_registry = GfxResourceRegistry::new();
            let cmd_allocator = CmdAllocator::new(gfx.device_ctx(), gfx.device_info_ctx());

            // 初始值应该是 1，因为 timeline semaphore 初始值是 0
            let init_frame_id = 1;
            let frame_rate_limit_fps = NonZeroU32::new(DefaultRenderRuntimeSettings::DEFAULT_FRAME_RATE_LIMIT_FPS)
                .expect("default frame rate limit must be non-zero");
            let min_frame_interval = Duration::from_secs_f64(1.0 / f64::from(frame_rate_limit_fps.get()));
            let frame_timing = FrameTiming::new(init_frame_id, Some(min_frame_interval));
            let shader_binding_system = ShaderBindingSystem::new(gfx.device_ctx(), frame_timing.frame_id());

            (gfx_resource_registry, cmd_allocator, frame_timing, shader_binding_system)
        };

        let mut world = {
            let _span = tracy_client::span!("RenderRuntime::new/world");
            GameWorld::new()
        };
        let default_sky_texture = {
            let _span = tracy_client::span!("RenderRuntime::new/default_sky_texture");
            world
                .import_texture(TruvisPath::resources_path("sky.jpg"), truvis_asset::handle::TextureColorSpace::Linear)
                .expect("failed to register default sky texture")
        };
        world.update_sky_texture(Some(default_sky_texture)).expect("failed to assign default sky texture");
        let render_world = {
            let _span = tracy_client::span!("RenderRuntime::new/render_world");
            RenderWorld::new(
                gfx.resource_ctx(),
                gfx.device_ctx(),
                gfx.immediate_ctx(),
                gfx.queue_ctx(),
                &mut gfx_resource_registry,
                &mut shader_binding_system,
                frame_timing.frame_id(),
            )
        };

        let ray_cast_service = {
            let _span = tracy_client::span!("RenderRuntime::new/ray_cast_service");
            RayCastService::new(
                gfx.resource_ctx(),
                gfx.device_ctx(),
                gfx.device_info_ctx(),
                gfx.queue_ctx(),
                shader_binding_system.global_descriptor_sets(),
            )
        };

        let per_frame_gpu_data = {
            let _span = tracy_client::span!("RenderRuntime::new/per_frame_gpu_data");
            PerFrameGpuData::new(gfx.resource_ctx())
        };

        let cmds = {
            let _span = tracy_client::span!("RenderRuntime::new/render_world_update_cmds");
            FrameLabel::ALL
                .into_iter()
                .map(|frame_label| {
                    cmd_allocator.alloc_command_buffer(gfx.device_ctx(), frame_label, "render-world-update")
                })
                .collect()
        };

        {
            let _span = tracy_client::span!("RenderRuntime::new/assemble_state");
            Self {
                gfx,
                cmd_allocator,
                fif_timeline_semaphore,
                render_world_update_cmds: cmds,
                swapchain_presenter: None,
                world,
                ray_cast_service,
                render_world,
                gfx_resource_registry,
                shader_binding_system,
                frame_timing,
                per_frame_gpu_data,
                frame_state,
                requested_render_extent: vk::Extent2D { width: 400, height: 400 },
                history_invalidated: false,
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
    /// runtime 在 Renderer/子系统 shutdown 前调用它，确保上层持有的 pipeline、descriptor、buffer
    /// 等资源被释放时，不会仍被上一帧 command buffer 引用。
    pub fn wait_idle(&self) {
        self.gfx.wait_idel();
    }

    /// 销毁 runtime 拥有的所有 GPU/CPU 子资源，并最后销毁 `Gfx` root owner。
    ///
    /// 调用前应已经完成 Renderer/子系统 shutdown。销毁顺序刻意从依赖 `Gfx` 的子资源开始，
    /// 先释放 present/asset/RenderWorld/command/descriptor 等对象，最后销毁 `Gfx`，
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
                &mut self.gfx_resource_registry,
            );
        }

        self.ray_cast_service.destroy_mut(self.gfx.resource_ctx(), self.gfx.device_ctx());
        // CPU scene/asset 与 render-side bridge 按依赖方向释放：先停止 scene runtime，
        // 再由 RenderWorld 释放场景缓存和其内部的 GPU asset resources。
        self.world.destroy_scene_mut();
        self.world.destroy();
        self.render_world.destroy(
            self.gfx.resource_ctx(),
            self.gfx.device_ctx(),
            &mut self.shader_binding_system,
            &mut self.gfx_resource_registry,
        );
        // per-frame UBO 与 command allocator 在所有使用它们的 scene/present 资源之后释放。
        self.per_frame_gpu_data.destroy(self.gfx.resource_ctx());
        self.render_world_update_cmds.clear();
        self.cmd_allocator.destroy(self.gfx.device_ctx());
        self.gfx_resource_registry.destroy(self.gfx.resource_ctx(), self.gfx.device_ctx());
        self.fif_timeline_semaphore.destroy(self.gfx.device_ctx());
        // descriptor/sampler 依赖 device 但不依赖业务资源，放在资源管理器之后、Gfx 之前销毁。
        self.shader_binding_system.destroy(self.gfx.device_ctx());
        self.gfx.destroy();
    }
}
// ---------------------------------------------------------------------------
// 生命周期方法（public API）
// ---------------------------------------------------------------------------
impl RenderRuntime {
    /// 自包含的帧开始流程：时间快照更新、FIF 等待、资源清理和 manager frame id 推进。
    ///
    /// 这里是 runtime 每帧唯一的资源回收入口。先等待当前 FIF 槽位不再被 GPU 使用，
    /// 再重置命令池和延迟释放队列；`GameWorld::poll_asset_loads` 在 update 之后的 prepare 边界执行。
    pub fn begin_frame(&mut self) {
        let _span = tracy_client::span!("RenderRuntime::begin_frame");
        self.history_invalidated = false;
        self.frame_timing.begin_frame();

        {
            let _span = tracy_client::span!("wait fif timeline");
            let current_frame_id = self.frame_timing.frame_id();
            let fif_count = FrameLabel::COUNT;
            let wait_frame_id = current_frame_id.saturating_sub(fif_count as u64);
            const WAIT_SEMAPHORE_TIMEOUT_NS: u64 = 30 * 1000 * 1000 * 1000;
            // 等待当前 frame label 上一次被使用的提交完成。这个等待是后续 reset command pool、
            // immediate release 和延迟释放队列清理的安全前提。
            self.fif_timeline_semaphore.wait_timeline(self.gfx.device_ctx(), wait_frame_id, WAIT_SEMAPHORE_TIMEOUT_NS);
        }

        {
            // command allocator 和 resource manager 都以 frame label/frame id 作为回收边界；
            // 上面的 timeline wait 确保不会重置 GPU 仍在读取的命令池或资源。
            self.cmd_allocator.reset_frame_commands(self.gfx.device_ctx(), self.frame_timing.frame_label());
            self.gfx_resource_registry.cleanup(
                self.gfx.resource_ctx(),
                self.gfx.device_ctx(),
                self.frame_timing.frame_id(),
            );
        }

        let current_frame_id = self.frame_timing.frame_id();
        // bindless 与 RenderWorld 内部的资源/场景 manager 都使用同一个 frame id 推进
        // 延迟回收窗口，保持 shader-visible slot 与 handle 的复用节奏一致。
        self.shader_binding_system.begin_frame(current_frame_id);
        self.render_world.begin_frame(current_frame_id);
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
            device_ctx: self.gfx.device_ctx(),
            world: &mut self.world,
            frame_state: &self.frame_state,
            swapchain_extent: self.frame_state.output_extent,
            frame_timing: &self.frame_timing,
            requested_render_extent: &mut self.requested_render_extent,
        }
    }

    /// 更新累积帧跟踪，并上传 GPU scene/descriptor 数据。
    ///
    /// 这是 update 与 render 之间的语义翻译边界：Renderer 仍拥有 camera/input state，
    /// runtime 只读取 render view 快照，并把 `GameWorld`、asset/material/instance bridge 的状态整理成
    /// render pass 可读取的 `RenderSceneView`。
    pub fn prepare(&mut self, input: &RenderFrameInput) {
        let _span = tracy_client::span!("RenderRuntime::prepare");

        self.prepare_render_world(input);
        self.update_perframe_descriptor_set();
    }

    /// prepare 后、render graph 组图前的 Renderer 同步查询阶段。
    ///
    /// 此阶段只暴露同步 raycast 能力。GPU scene/TLAS 已提交到 graphics queue，后续
    /// raycast 提交会通过同队列顺序看到 prepare 结果，并用自身 fence 阻塞读回。
    pub fn ray_cast_phase(&mut self) -> RenderRuntimeRayCastCtx<'_> {
        RenderRuntimeRayCastCtx {
            device_ctx: self.gfx.device_ctx(),
            resource_ctx: self.gfx.resource_ctx(),
            queue_ctx: self.gfx.queue_ctx(),
            frame_timing: &self.frame_timing,
            shader_bindings: self.shader_binding_system.view(),
            render_scene: &self.render_world,
            render_instance_table: self.render_world.render_instance_table(),
            ray_cast_service: &mut self.ray_cast_service,
            history_invalidated: self.history_invalidated,
        }
    }

    /// 共享借用：render 阶段中 RenderRuntime 状态只读。
    ///
    /// 这个 Ctx 面向 RenderGraph/pass 录制。它故意不暴露 `GameWorld` 的可变借用，避免 render
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
            record_ctx: RenderPassRecordCtx {
                frame_timing: &self.frame_timing,
                frame_state: &self.frame_state,
                shader_bindings: self.shader_binding_system.view(),
                gfx_resource_registry: &self.gfx_resource_registry,
                per_frame_gpu_data: &self.per_frame_gpu_data,
            },
            render_scene: &self.render_world,
            world_submesh_raster: &self.render_world,
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

    pub fn frame_state(&self) -> &FrameRenderState {
        &self.frame_state
    }

    /// 应用 Renderer 提交的内部渲染尺寸。
    ///
    /// Runtime 只比较并写入中性的 `FrameRenderState`；尺寸来源和其派生策略属于 Renderer。
    pub fn sync_render_extent(&mut self) -> Option<RenderRuntimeResizeCtx<'_>> {
        let output_extent = self.swapchain_presenter.as_ref().unwrap().extent();
        let new_render_extent = self.requested_render_extent;
        let changed = self.frame_state.output_extent != output_extent
            || self.frame_state.render_extent != new_render_extent;
        if !changed {
            return None;
        }

        self.gfx.wait_idel();
        self.frame_state.output_extent = output_extent;
        self.frame_state.render_extent = new_render_extent;
        self.history_invalidated = true;
        self.render_world.request_motion_history_reset();

        Some(RenderRuntimeResizeCtx {
            device_ctx: self.gfx.device_ctx(),
            resource_ctx: self.gfx.resource_ctx(),
            immediate_ctx: self.gfx.immediate_ctx(),
            surface_ctx: self.gfx.surface_ctx(),
            gfx_resource_registry: &mut self.gfx_resource_registry,
            shader_binding_system: &mut self.shader_binding_system,
            frame_timing: &self.frame_timing,
            frame_state: &mut self.frame_state,
            present: self.swapchain_presenter.as_ref().unwrap().view(),
            requested_render_extent: &mut self.requested_render_extent,
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
    /// render graph，但 frame id 仍需要前进；提交一个空 signal 可以保持后续
    /// `begin_frame` 对 timeline 的等待不会落到永远无人 signal 的 frame id 上。
    pub fn signal_current_frame_complete(&self) {
        let frame_id = self.frame_timing.frame_id();
        let submit_info = GfxSubmitInfo::new(&[]).signal(
            &self.fif_timeline_semaphore,
            vk::PipelineStageFlags2::BOTTOM_OF_PIPE,
            Some(frame_id),
        );
        self.gfx.queue_ctx().gfx_queue().submit(vec![submit_info], None);
    }

    /// 推进帧计数器。
    ///
    /// 所有按 `FrameLabel` 轮转的资源都在此之后切到下一帧标签；因此必须放在
    /// present 之后，作为本帧生命周期的最后一步。
    pub fn end_frame(&mut self) {
        let _span = tracy_client::span!("RenderRuntime::end_frame");
        self.frame_timing.next_frame();
    }

    /// 查询距离下一帧渲染时机的剩余时间。
    ///
    /// `None` 表示未启用软件限帧，`Some(Duration::ZERO)` 表示已经到达时机。
    /// 该方法只做时间计算，不推进 frame id，也不会等待 GPU。
    pub fn remaining_until_render(&self) -> Option<Duration> {
        self.frame_timing.remaining_until_render()
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
            &mut self.gfx_resource_registry,
        );
        // runtime 只同步 frame state；具体 RT / main-view / GBuffer target 的重建由随后
        // 返回的 resize ctx 交给 Renderer/子系统完成，避免 engine 反向持有管线策略资源。
        self.sync_frame_extent_after_present_resize();

        Some(RenderRuntimeResizeCtx {
            device_ctx: self.gfx.device_ctx(),
            resource_ctx: self.gfx.resource_ctx(),
            immediate_ctx: self.gfx.immediate_ctx(),
            surface_ctx: self.gfx.surface_ctx(),
            gfx_resource_registry: &mut self.gfx_resource_registry,
            shader_binding_system: &mut self.shader_binding_system,
            frame_timing: &self.frame_timing,
            frame_state: &mut self.frame_state,
            present: self.swapchain_presenter.as_ref().unwrap().view(),
            requested_render_extent: &mut self.requested_render_extent,
        })
    }

    /// 生成 shutdown 阶段上下文，供 Renderer/子系统在 runtime 子资源销毁前释放自己持有的 GPU 资源。
    ///
    /// 这个阶段仍暴露 GPU 资源/binding owner 与 `CmdAllocator` 的可变借用，但不再允许继续进入 update/render
    /// 帧流程；调用者应在 `wait_idle` 后使用它清理长期资源，再让 `destroy` 接管 runtime-owned 资源。
    pub fn shutdown_phase(&mut self) -> RenderRuntimeShutdownCtx<'_> {
        RenderRuntimeShutdownCtx {
            device_ctx: self.gfx.device_ctx(),
            resource_ctx: self.gfx.resource_ctx(),
            queue_ctx: self.gfx.queue_ctx(),
            immediate_ctx: self.gfx.immediate_ctx(),
            surface_ctx: self.gfx.surface_ctx(),
            gfx_resource_registry: &mut self.gfx_resource_registry,
            shader_binding_system: &mut self.shader_binding_system,
            frame_timing: &self.frame_timing,
            frame_state: &self.frame_state,
            cmd_allocator: &mut self.cmd_allocator,
        }
    }

    /// window/surface 创建后的一次性初始化。返回用于 Renderer/子系统初始化的上下文。
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
            &mut self.gfx_resource_registry,
            raw_display_handle,
            raw_window_handle,
            vk::Extent2D {
                width: window_physical_size[0],
                height: window_physical_size[1],
            },
        ));
        // surface 创建后才能知道平台裁剪后的实际 swapchain extent。这里先同步到
        // `frame_state`，让后续 Renderer 子系统创建 renderer-owned target 时拿到真实尺寸。
        self.sync_frame_extent_after_present_resize();

        RenderRuntimeInitCtx {
            device_ctx: self.gfx.device_ctx(),
            resource_ctx: self.gfx.resource_ctx(),
            queue_ctx: self.gfx.queue_ctx(),
            device_info_ctx: self.gfx.device_info_ctx(),
            immediate_ctx: self.gfx.immediate_ctx(),
            surface_ctx: self.gfx.surface_ctx(),
            world: &mut self.world,
            gfx_resource_registry: &mut self.gfx_resource_registry,
            shader_binding_system: &mut self.shader_binding_system,
            frame_timing: &self.frame_timing,
            frame_state: &mut self.frame_state,
            cmd_allocator: &mut self.cmd_allocator,
            swapchain_image_info: self.swapchain_presenter.as_ref().unwrap().swapchain_image_info(),
            present: self.swapchain_presenter.as_ref().unwrap().view(),
            requested_render_extent: &mut self.requested_render_extent,
        }
    }
}

// ---------------------------------------------------------------------------
// Prepare 数据上传
// ---------------------------------------------------------------------------
impl RenderRuntime {
    /// 根据 app render view 快照更新 main view 累积帧计数。
    ///
    /// 累积渲染关心最终视图/投影是否变化；后续 pass 根据这里的计数决定是否复用上一帧结果。
    /// 准备 render pass 可见的 GPU scene 与 per-frame uniform。
    ///
    /// 该函数把所有 staging copy 录到同一个 command buffer，最后一次提交到 graphics queue；
    /// render graph 在后续命令提交中通过常规 queue 顺序看到这些写入。
    fn prepare_render_world(&mut self, input: &RenderFrameInput) {
        let _span = tracy_client::span!("RenderRuntime::prepare_render_world");
        let frame_extent = self.frame_state.render_extent;
        let frame_label = self.frame_timing.frame_label();
        let cmd = self.render_world_update_cmds[*frame_label].clone();

        // GameWorld 同步必须发生在 Renderer update 之后、RenderWorld buffer 上传之前。
        // loader completion 先收敛到 CPU registry；RenderWorld 随后通过内部
        // RenderAssetSystem 从最终资源表对账并提交尚未安装的 texture/mesh，再由 bindless prepare 写入本帧 descriptor。
        self.world.poll_asset_loads();
        let scene_view = self.world.scene_view();
        let resource_sync_result = self.render_world.sync_assets(
            scene_view,
            self.gfx.resource_ctx(),
            self.gfx.device_ctx(),
            self.gfx.queue_ctx(),
            &mut self.gfx_resource_registry,
            &mut self.shader_binding_system,
        );

        // RenderWorld 更新使用独立命令缓冲，把 material/instance/geometry/light/scene buffer
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

        // bindless 表先更新，因为 material upload 和环境绑定都可能立即解析 texture SRV handle；
        // 后续 scene root buffer 会写入这些 shader-visible handle。
        self.shader_binding_system.prepare_render_data(self.gfx.device_ctx(), &self.gfx_resource_registry);
        let render_world_result = self.render_world.prepare_render_data(
            self.gfx.resource_ctx(),
            self.gfx.device_ctx(),
            self.gfx.immediate_ctx(),
            &cmd,
            transfer_barrier_mask,
            self.frame_timing.frame_id(),
            self.frame_timing.frame_label(),
            scene_view,
            resource_sync_result,
        );
        if render_world_result.history_invalidated {
            self.history_invalidated = true;
            self.render_world.request_motion_history_reset();
        }

        // per-frame uniform 放在 GPU scene 上传之后写入同一条命令缓冲，保证本帧 shader
        // 看到的相机、分辨率、时间和 scene buffer 都来自同一个 prepare 快照。
        let previous_view = input.previous_view;
        let render_view = input.render_view;
        let temporal_jitter_px = input.temporal_jitter_px;
        let per_frame_data = gpu::engine::frame::PerFrameData {
            projection: render_view.projection.into(),
            view: render_view.view.into(),
            inv_view: render_view.inv_view.into(),
            inv_projection: render_view.inv_projection.into(),
            prev_view: previous_view.view.into(),
            prev_projection: previous_view.projection.into(),
            camera_pos: render_view.position_ws.into(),
            camera_forward: render_view.forward_ws.into(),
            time_ms: self.frame_timing.total_time_ms(),
            delta_time_ms: self.frame_timing.delta_time_ms(),
            frame_id: self.frame_timing.frame_id(),
            resolution: gpu::Float2 {
                x: frame_extent.width as f32,
                y: frame_extent.height as f32,
            },
            temporal_jitter_px: gpu::Float2 {
                x: temporal_jitter_px[0],
                y: temporal_jitter_px[1],
            },
            // 主流程已不再做 progressive accumulation；保持为 0 可以让 raygen 每帧稳定写入当前图像。
            accum_frames: 0,
            _padding_0: Default::default(),
            _padding_1: Default::default(),
            _padding_2: Default::default(),
        };
        self.per_frame_gpu_data.write(frame_label, &cmd, per_frame_data);
        cmd.buffer_memory_barrier(
            vk::DependencyFlags::empty(),
            &[GfxBufferBarrier::default()
                .buffer(self.per_frame_gpu_data.buffer(frame_label).vk_buffer(), 0, vk::WHOLE_SIZE)
                .mask(transfer_barrier_mask)],
        );
        cmd.end();
        self.gfx.queue_ctx().gfx_queue().submit(vec![GfxSubmitInfo::new(std::slice::from_ref(&cmd))], None);
        self.render_world.commit_submitted_frame(frame_label);
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
            .acquire_image(self.gfx.surface_ctx(), self.frame_timing.frame_label())
    }

    /// 同步 swapchain extent 到 `FrameRenderState`。
    ///
    /// present 层负责判断 swapchain 是否需要重建；具体窗口尺寸 render target
    /// 属于 Renderer/子系统 owner，这里只维护 runtime 的 frame state。
    fn update_frame_state(&mut self) {
        self.sync_frame_extent_after_present_resize();
    }

    /// 同步 present extent 到 runtime frame state。
    fn sync_frame_extent_after_present_resize(&mut self) {
        let swapchain_extent = self.swapchain_presenter.as_ref().unwrap().extent();
        if self.frame_state.output_extent == swapchain_extent {
            return;
        }
        self.frame_state.output_extent = swapchain_extent;
        self.requested_render_extent = swapchain_extent;
        self.frame_state.render_extent = swapchain_extent;
        self.history_invalidated = true;
        self.render_world.request_motion_history_reset();
    }

    /// 刷新当前 FIF per-frame descriptor set。
    ///
    /// descriptor 指向刚写入的 per-frame UBO 和 shader ABI `gpu::engine::scene::GpuScene` root buffer；render pass
    /// 通过全局 descriptor set 读取本帧相机、时间与 scene device address。
    fn update_perframe_descriptor_set(&mut self) {
        let frame_label = self.frame_timing.frame_label();
        let per_frame_data_buffer = self.per_frame_gpu_data.buffer(frame_label);
        let gpu_scene_buffer = self.render_world.scene_buffer(frame_label);
        let perframe_set =
            self.shader_binding_system.global_descriptor_sets().current_perframe_set(frame_label).handle();

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
