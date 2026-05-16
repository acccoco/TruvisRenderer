use std::rc::Rc;

use ash::vk;
use itertools::Itertools;

use truvis_gfx::descriptors::descriptor::{GfxDescriptorSet, GfxDescriptorSetLayout};
use truvis_gfx::descriptors::descriptor_pool::{GfxDescriptorPool, GfxDescriptorPoolCreateInfo};
use truvis_gfx::gfx::GfxDeviceCtx;

pub use crate::descriptor_bindings::{
    BindlessDescriptorBinding, BindlessDescriptorTarget, PerFrameDescriptorBinding, StaticDescriptorBinding,
    StaticSamplerDescriptorTarget,
};
use crate::frame_counter::FrameCounter;
use crate::pipeline_settings::FrameLabel;

pub struct GlobalDescriptorSets {
    layout_0_static: GfxDescriptorSetLayout<StaticDescriptorBinding>,
    set_0_static: GfxDescriptorSet<StaticDescriptorBinding>,

    layout_1_bindless: GfxDescriptorSetLayout<BindlessDescriptorBinding>,
    // 单套 bindless descriptor set，配合 UPDATE_UNUSED_WHILE_PENDING_BIT 使用
    set_1_bindless: GfxDescriptorSet<BindlessDescriptorBinding>,

    layout_2_perframe: GfxDescriptorSetLayout<PerFrameDescriptorBinding>,
    set_2_perframe: [GfxDescriptorSet<PerFrameDescriptorBinding>; FrameCounter::fif_count()],

    _descriptor_pool: GfxDescriptorPool,
}
// 创建与初始化
impl GlobalDescriptorSets {
    pub fn new(ctx: GfxDeviceCtx<'_>) -> Self {
        let descriptor_pool = Self::init_descriptor_pool(ctx);

        let layout_0_static = GfxDescriptorSetLayout::<StaticDescriptorBinding>::new(
            ctx,
            vk::DescriptorSetLayoutCreateFlags::UPDATE_AFTER_BIND_POOL,
            "global-layout",
        );
        let set_0_static = GfxDescriptorSet::<StaticDescriptorBinding>::new(
            ctx,
            &descriptor_pool,
            &layout_0_static,
            "global-descriptor-set",
        );

        let layout_1_bindless = GfxDescriptorSetLayout::<BindlessDescriptorBinding>::new(
            ctx,
            vk::DescriptorSetLayoutCreateFlags::UPDATE_AFTER_BIND_POOL,
            "bindless-layout",
        );
        let set_1_bindless = GfxDescriptorSet::<BindlessDescriptorBinding>::new(
            ctx,
            &descriptor_pool,
            &layout_1_bindless,
            "bindless-descriptor-set",
        );

        let layout_2_perframe = GfxDescriptorSetLayout::<PerFrameDescriptorBinding>::new(
            ctx,
            vk::DescriptorSetLayoutCreateFlags::empty(),
            "perframe-layout",
        );
        let set_2_perframe = FrameCounter::frame_labes().map(|frame_label| {
            GfxDescriptorSet::<PerFrameDescriptorBinding>::new(
                ctx,
                &descriptor_pool,
                &layout_2_perframe,
                format!("perframe-descriptor-set-{frame_label}"),
            )
        });

        Self {
            layout_0_static,
            set_0_static,

            layout_1_bindless,
            set_1_bindless,

            layout_2_perframe,
            set_2_perframe,

            _descriptor_pool: descriptor_pool,
        }
    }

    fn init_descriptor_pool(ctx: GfxDeviceCtx<'_>) -> GfxDescriptorPool {
        let pool_size = [
            (vk::DescriptorType::COMBINED_IMAGE_SAMPLER, 512),
            (vk::DescriptorType::STORAGE_IMAGE, 512),
            (vk::DescriptorType::SAMPLED_IMAGE, 512),
            (vk::DescriptorType::SAMPLER, 32),
            (vk::DescriptorType::UNIFORM_BUFFER, 32),
        ]
        .iter()
        .map(|(ty, count)| vk::DescriptorPoolSize {
            ty: *ty,
            descriptor_count: *count,
        })
        .collect_vec();

        let pool_ci = Rc::new(GfxDescriptorPoolCreateInfo::new(
            vk::DescriptorPoolCreateFlags::FREE_DESCRIPTOR_SET | vk::DescriptorPoolCreateFlags::UPDATE_AFTER_BIND,
            32,
            pool_size,
        ));

        GfxDescriptorPool::new(ctx, pool_ci, "render-backend")
    }
}
impl Default for GlobalDescriptorSets {
    fn default() -> Self {
        panic!("GlobalDescriptorSets::default requires explicit Gfx Ctx; use GlobalDescriptorSets::new")
    }
}
// 销毁
impl GlobalDescriptorSets {
    pub fn destroy(self, ctx: GfxDeviceCtx<'_>) {
        let Self {
            layout_0_static,
            set_0_static: _,
            layout_1_bindless,
            set_1_bindless: _,
            layout_2_perframe,
            set_2_perframe: _,
            _descriptor_pool,
        } = self;
        layout_0_static.destroy(ctx);
        layout_1_bindless.destroy(ctx);
        layout_2_perframe.destroy(ctx);
        _descriptor_pool.destroy(ctx);
    }
}
// 访问器
impl GlobalDescriptorSets {
    #[inline]
    pub fn static_sampler_target(&self) -> StaticSamplerDescriptorTarget {
        StaticSamplerDescriptorTarget {
            set: self.set_0_static.handle(),
        }
    }

    #[inline]
    pub fn bindless_target(&self) -> BindlessDescriptorTarget {
        BindlessDescriptorTarget {
            set: self.set_1_bindless.handle(),
        }
    }

    #[inline]
    pub fn sampler_set(&self) -> &GfxDescriptorSet<StaticDescriptorBinding> {
        &self.set_0_static
    }

    #[inline]
    pub fn bindless_set(&self) -> &GfxDescriptorSet<BindlessDescriptorBinding> {
        &self.set_1_bindless
    }

    #[inline]
    pub fn current_perframe_set(&self, frame_label: FrameLabel) -> &GfxDescriptorSet<PerFrameDescriptorBinding> {
        &self.set_2_perframe[*frame_label]
    }

    #[inline]
    pub fn global_set_layouts(&self) -> Vec<vk::DescriptorSetLayout> {
        vec![
            self.layout_0_static.handle(),
            self.layout_1_bindless.handle(),
            self.layout_2_perframe.handle(),
        ]
    }

    #[inline]
    pub fn global_sets(&self, frame_label: FrameLabel) -> Vec<vk::DescriptorSet> {
        vec![
            self.set_0_static.handle(),
            self.set_1_bindless.handle(),
            self.set_2_perframe[*frame_label].handle(),
        ]
    }
}
