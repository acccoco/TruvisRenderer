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
    #[stage = "FRAGMENT | RAYGEN_KHR | CLOSEST_HIT_KHR | ANY_HIT_KHR | MISS_KHR | COMPUTE"]
    #[count = "truvis_shader_binding::gpu::engine::bindless::ESamplerType__Count_"]
    #[flags = "PARTIALLY_BOUND | UPDATE_AFTER_BIND"]
    _samplers: (),
}

#[derive(DescriptorBinding)]
pub struct BindlessDescriptorBinding {
    #[binding = 0]
    #[descriptor_type = "SAMPLED_IMAGE"]
    #[stage = "FRAGMENT | RAYGEN_KHR | CLOSEST_HIT_KHR | ANY_HIT_KHR | MISS_KHR | COMPUTE"]
    // 大型场景可能包含数百张材质贴图；保持固定 descriptor set 和稳定 slot，无需运行时重建 layout。
    #[count = 1024]
    #[flags = "PARTIALLY_BOUND | UPDATE_AFTER_BIND | UPDATE_UNUSED_WHILE_PENDING"]
    _srvs: (),
}

impl BindlessDescriptorBinding {
    pub fn descriptor_count() -> usize {
        Self::srvs().count as usize
    }
}

#[derive(DescriptorBinding)]
pub struct PerFrameDescriptorBinding {
    #[binding = 0]
    #[descriptor_type = "UNIFORM_BUFFER"]
    #[stage = "VERTEX | FRAGMENT | RAYGEN_KHR | CLOSEST_HIT_KHR | ANY_HIT_KHR | MISS_KHR | COMPUTE"]
    #[count = 1]
    _per_frame_data: (),

    #[binding = 1]
    #[descriptor_type = "UNIFORM_BUFFER"]
    #[stage = "VERTEX | FRAGMENT | RAYGEN_KHR | CLOSEST_HIT_KHR | ANY_HIT_KHR | MISS_KHR | COMPUTE"]
    #[count = 1]
    _gpu_scene: (),
}
