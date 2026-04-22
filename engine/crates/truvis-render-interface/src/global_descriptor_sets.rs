use std::rc::Rc;

use ash::vk;
use itertools::Itertools;

use truvis_descriptor_layout_macro::DescriptorBinding;
use truvis_gfx::descriptors::descriptor::{GfxDescriptorSet, GfxDescriptorSetLayout};
use truvis_gfx::descriptors::descriptor_pool::{GfxDescriptorPool, GfxDescriptorPoolCreateInfo};

use crate::frame_counter::FrameCounter;
use crate::pipeline_settings::FrameLabel;

#[derive(DescriptorBinding)]
pub struct StaticDescriptorBinding {
    #[binding = 0]
    #[descriptor_type = "SAMPLER"]
    #[stage = "FRAGMENT | RAYGEN_KHR | CLOSEST_HIT_KHR | ANY_HIT_KHR | CALLABLE_KHR | MISS_KHR | COMPUTE"]
    #[count = 32]
    #[flags = "PARTIALLY_BOUND | UPDATE_AFTER_BIND"]
    _samplers: (),
}

#[derive(DescriptorBinding)]
pub struct BindlessDescriptorBinding {
    #[binding = 0]
    #[descriptor_type = "COMBINED_IMAGE_SAMPLER"]
    #[stage = "FRAGMENT | RAYGEN_KHR | CLOSEST_HIT_KHR | ANY_HIT_KHR | CALLABLE_KHR | MISS_KHR | COMPUTE"]
    #[count = 128]
    #[flags = "PARTIALLY_BOUND | UPDATE_AFTER_BIND | UPDATE_UNUSED_WHILE_PENDING"]
    _textures: (),

    #[binding = 1]
    #[descriptor_type = "STORAGE_IMAGE"]
    #[stage = "FRAGMENT | RAYGEN_KHR | CLOSEST_HIT_KHR | ANY_HIT_KHR | CALLABLE_KHR | MISS_KHR | COMPUTE"]
    #[count = 128]
    #[flags = "PARTIALLY_BOUND | UPDATE_AFTER_BIND | UPDATE_UNUSED_WHILE_PENDING"]
    _uavs: (),

    #[binding = 2]
    #[descriptor_type = "SAMPLED_IMAGE"]
    #[stage = "FRAGMENT | RAYGEN_KHR | CLOSEST_HIT_KHR | ANY_HIT_KHR | CALLABLE_KHR | MISS_KHR | COMPUTE"]
    #[count = 128]
    #[flags = "PARTIALLY_BOUND | UPDATE_AFTER_BIND | UPDATE_UNUSED_WHILE_PENDING"]
    _srvs: (),
}

#[derive(DescriptorBinding)]
pub struct PerFrameDescriptorBinding {
    #[binding = 0]
    #[descriptor_type = "UNIFORM_BUFFER"]
    #[stage = "FRAGMENT | RAYGEN_KHR | CLOSEST_HIT_KHR | ANY_HIT_KHR | CALLABLE_KHR | MISS_KHR | COMPUTE"]
    #[count = 1]
    _per_frame_data: (),

    #[binding = 1]
    #[descriptor_type = "UNIFORM_BUFFER"]
    #[stage = "FRAGMENT | RAYGEN_KHR | CLOSEST_HIT_KHR | ANY_HIT_KHR | CALLABLE_KHR | MISS_KHR | COMPUTE"]
    #[count = 1]
    _gpu_scene: (),
}

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
// new & init
impl GlobalDescriptorSets {
    pub fn new() -> Self {
        let descriptor_pool = Self::init_descriptor_pool();

        let layout_0_static = GfxDescriptorSetLayout::<StaticDescriptorBinding>::new(
            vk::DescriptorSetLayoutCreateFlags::UPDATE_AFTER_BIND_POOL,
            "global-layout",
        );
        let set_0_static = GfxDescriptorSet::<StaticDescriptorBinding>::new(
            &descriptor_pool,
            &layout_0_static,
            "global-descriptor-set",
        );

        let layout_1_bindless = GfxDescriptorSetLayout::<BindlessDescriptorBinding>::new(
            vk::DescriptorSetLayoutCreateFlags::UPDATE_AFTER_BIND_POOL,
            "bindless-layout",
        );
        let set_1_bindless = GfxDescriptorSet::<BindlessDescriptorBinding>::new(
            &descriptor_pool,
            &layout_1_bindless,
            "bindless-descriptor-set",
        );

        let layout_2_perframe = GfxDescriptorSetLayout::<PerFrameDescriptorBinding>::new(
            vk::DescriptorSetLayoutCreateFlags::empty(),
            "perframe-layout",
        );
        let set_2_perframe = FrameCounter::frame_labes().map(|frame_label| {
            GfxDescriptorSet::<PerFrameDescriptorBinding>::new(
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

    fn init_descriptor_pool() -> GfxDescriptorPool {
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

        GfxDescriptorPool::new(pool_ci, "renderer")
    }
}
impl Default for GlobalDescriptorSets {
    fn default() -> Self {
        Self::new()
    }
}
// destroy
impl GlobalDescriptorSets {
    pub fn destroy_mut(&mut self) {
        // descriptor sets 跟随 pool 一起销毁
    }
    pub fn destroy(self) {
        // descriptor sets 跟随 pool 一起销毁
    }
}
impl Drop for GlobalDescriptorSets {
    fn drop(&mut self) {
        self.destroy_mut();
    }
}
// getters
impl GlobalDescriptorSets {
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
