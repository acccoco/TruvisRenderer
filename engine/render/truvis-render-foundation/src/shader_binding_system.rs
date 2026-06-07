use ash::vk;

use truvis_gfx::descriptors::descriptor::GfxDescriptorSet;
use truvis_gfx::gfx::GfxDeviceCtx;

use crate::bindless_manager::{BindlessManager, BindlessSrvHandle, BindlessUavHandle};
use crate::descriptor_bindings::{BindlessDescriptorBinding, PerFrameDescriptorBinding, StaticDescriptorBinding};
use crate::frame_counter::{FrameLabel, FrameToken};
use crate::gfx_resource_manager::GfxResourceManager;
use crate::global_descriptor_sets::GlobalDescriptorSets;
use crate::handles::GfxImageViewHandle;
use crate::sampler_manager::RenderSamplerManager;

/// shader-visible binding 系统的长期 owner。
///
/// 它只负责全局 descriptor set、bindless slot 和静态 sampler 的生命周期与更新。
/// 资源对象本身仍由 `GfxResourceManager` 持有；这里在刷新 bindless descriptor 时
/// 只临时借用资源 manager 查询 image view。
pub struct ShaderBindingSystem {
    global_descriptor_sets: GlobalDescriptorSets,
    bindless_manager: BindlessManager,
    sampler_manager: RenderSamplerManager,
}

impl ShaderBindingSystem {
    pub fn new(ctx: GfxDeviceCtx<'_>, initial_frame_token: FrameToken) -> Self {
        let _span = tracy_client::span!("ShaderBindingSystem::new");
        let global_descriptor_sets = GlobalDescriptorSets::new(ctx);
        let sampler_manager = RenderSamplerManager::new(ctx, global_descriptor_sets.static_sampler_target());
        let bindless_manager = BindlessManager::new(initial_frame_token);

        Self {
            global_descriptor_sets,
            bindless_manager,
            sampler_manager,
        }
    }

    pub fn destroy(self, ctx: GfxDeviceCtx<'_>) {
        let Self {
            global_descriptor_sets,
            bindless_manager: _,
            sampler_manager,
        } = self;
        sampler_manager.destroy(ctx);
        global_descriptor_sets.destroy(ctx);
    }

    #[inline]
    pub fn begin_frame(&mut self, frame_token: FrameToken) {
        self.bindless_manager.begin_frame(frame_token);
    }

    #[inline]
    pub fn prepare_render_data(&mut self, ctx: GfxDeviceCtx<'_>, gfx_resource_manager: &GfxResourceManager) {
        let bindless_target = self.global_descriptor_sets.bindless_target();
        self.bindless_manager.prepare_render_data(ctx, gfx_resource_manager, bindless_target);
    }

    #[inline]
    pub fn register_srv(&mut self, image_view_handle: GfxImageViewHandle) {
        self.bindless_manager.register_srv(image_view_handle);
    }

    #[inline]
    pub fn unregister_srv(&mut self, image_view_handle: GfxImageViewHandle) {
        self.bindless_manager.unregister_srv(image_view_handle);
    }

    #[inline]
    pub fn register_uav(&mut self, image_view_handle: GfxImageViewHandle) {
        self.bindless_manager.register_uav(image_view_handle);
    }

    #[inline]
    pub fn unregister_uav(&mut self, image_view_handle: GfxImageViewHandle) {
        self.bindless_manager.unregister_uav(image_view_handle);
    }

    #[inline]
    pub fn get_shader_srv_handle(&self, image_view_handle: GfxImageViewHandle) -> BindlessSrvHandle {
        self.bindless_manager.get_shader_srv_handle(image_view_handle)
    }

    #[inline]
    pub fn get_shader_uav_handle(&self, image_view_handle: GfxImageViewHandle) -> BindlessUavHandle {
        self.bindless_manager.get_shader_uav_handle(image_view_handle)
    }

    #[inline]
    pub fn global_descriptor_sets(&self) -> &GlobalDescriptorSets {
        &self.global_descriptor_sets
    }

    #[inline]
    pub fn view(&self) -> ShaderBindingView<'_> {
        ShaderBindingView {
            global_descriptor_sets: &self.global_descriptor_sets,
            bindless_manager: &self.bindless_manager,
        }
    }
}

/// render/pass 阶段可见的只读 shader binding 视图。
///
/// 它允许 pass 查询全局 descriptor set 与 shader-visible bindless handle，
/// 但不允许注册、注销或刷新 descriptor。
#[derive(Clone, Copy)]
pub struct ShaderBindingView<'a> {
    global_descriptor_sets: &'a GlobalDescriptorSets,
    bindless_manager: &'a BindlessManager,
}

impl ShaderBindingView<'_> {
    #[inline]
    pub fn global_descriptor_sets(&self) -> &GlobalDescriptorSets {
        self.global_descriptor_sets
    }

    #[inline]
    pub fn bindless_manager(&self) -> &BindlessManager {
        self.bindless_manager
    }

    #[inline]
    pub fn global_set_layouts(&self) -> Vec<vk::DescriptorSetLayout> {
        self.global_descriptor_sets.global_set_layouts()
    }

    #[inline]
    pub fn global_sets(&self, frame_label: FrameLabel) -> Vec<vk::DescriptorSet> {
        self.global_descriptor_sets.global_sets(frame_label)
    }

    #[inline]
    pub fn current_perframe_set(&self, frame_label: FrameLabel) -> &GfxDescriptorSet<PerFrameDescriptorBinding> {
        self.global_descriptor_sets.current_perframe_set(frame_label)
    }

    #[inline]
    pub fn sampler_set(&self) -> &GfxDescriptorSet<StaticDescriptorBinding> {
        self.global_descriptor_sets.sampler_set()
    }

    #[inline]
    pub fn bindless_set(&self) -> &GfxDescriptorSet<BindlessDescriptorBinding> {
        self.global_descriptor_sets.bindless_set()
    }

    #[inline]
    pub fn get_shader_srv_handle(&self, image_view_handle: GfxImageViewHandle) -> BindlessSrvHandle {
        self.bindless_manager.get_shader_srv_handle(image_view_handle)
    }

    #[inline]
    pub fn get_shader_uav_handle(&self, image_view_handle: GfxImageViewHandle) -> BindlessUavHandle {
        self.bindless_manager.get_shader_uav_handle(image_view_handle)
    }
}
