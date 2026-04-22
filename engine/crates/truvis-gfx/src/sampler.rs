use std::hash::Hash;

use ash::vk;

use crate::gfx::Gfx;

// Sampler descriptor
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct GfxSamplerDesc {
    pub mag_filter: vk::Filter,
    pub min_filter: vk::Filter,
    pub address_mode_u: vk::SamplerAddressMode,
    pub address_mode_v: vk::SamplerAddressMode,
    pub address_mode_w: vk::SamplerAddressMode,
    pub max_anisotropy: u32,
    pub compare_op: Option<vk::CompareOp>,
    pub mipmap_mode: vk::SamplerMipmapMode,
}
impl Default for GfxSamplerDesc {
    fn default() -> Self {
        Self {
            mag_filter: vk::Filter::LINEAR,
            min_filter: vk::Filter::LINEAR,
            address_mode_u: vk::SamplerAddressMode::REPEAT,
            address_mode_v: vk::SamplerAddressMode::REPEAT,
            address_mode_w: vk::SamplerAddressMode::REPEAT,
            max_anisotropy: 0,
            compare_op: None,
            mipmap_mode: vk::SamplerMipmapMode::LINEAR,
        }
    }
}

pub struct GfxSampler {
    handle: vk::Sampler,
}
// new & init
impl GfxSampler {
    pub fn new(desc: &GfxSamplerDesc, name: impl AsRef<str>) -> Self {
        let mut create_info = vk::SamplerCreateInfo::default()
            .mag_filter(desc.mag_filter)
            .min_filter(desc.min_filter)
            .address_mode_u(desc.address_mode_u)
            .address_mode_v(desc.address_mode_v)
            .address_mode_w(desc.address_mode_w)
            .mipmap_mode(desc.mipmap_mode)
            .min_lod(0.0)
            .max_lod(vk::LOD_CLAMP_NONE)
            .border_color(vk::BorderColor::INT_OPAQUE_BLACK);

        if desc.max_anisotropy > 0 {
            create_info = create_info.anisotropy_enable(true).max_anisotropy(desc.max_anisotropy as f32);
        } else {
            create_info = create_info.anisotropy_enable(false);
        }

        if let Some(compare_op) = desc.compare_op {
            create_info = create_info.compare_enable(true).compare_op(compare_op);
        } else {
            create_info = create_info.compare_enable(false);
        }

        let sampler =
            unsafe { Gfx::get().gfx_device().create_sampler(&create_info, None).expect("Failed to create sampler") };
        Gfx::get().gfx_device().set_object_debug_name(sampler, name.as_ref());

        Self { handle: sampler }
    }
}
// getters
impl GfxSampler {
    #[inline]
    pub fn handle(&self) -> vk::Sampler {
        self.handle
    }
}
impl Drop for GfxSampler {
    fn drop(&mut self) {
        unsafe {
            Gfx::get().gfx_device().destroy_sampler(self.handle, None);
        }
    }
}
