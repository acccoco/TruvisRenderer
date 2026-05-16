use ash::vk;

use truvis_descriptor_layout_macro::DescriptorBinding;

#[derive(Copy, Clone)]
pub struct StaticSamplerDescriptorTarget {
    pub set: vk::DescriptorSet,
}

#[derive(Copy, Clone)]
pub struct BindlessDescriptorTarget {
    pub set: vk::DescriptorSet,
}

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

impl BindlessDescriptorBinding {
    pub fn descriptor_count() -> usize {
        let count = Self::srvs().count;
        debug_assert_eq!(Self::textures().count, count);
        debug_assert_eq!(Self::uavs().count, count);
        count as usize
    }
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
