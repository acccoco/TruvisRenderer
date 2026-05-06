use ash::vk;
use itertools::Itertools;

use truvis_gfx::gfx::GfxDeviceCtx;
use truvis_gfx::sampler::{GfxSampler, GfxSamplerDesc};
use truvis_gfx::utilities::descriptor_cursor::GfxDescriptorCursor;
use truvis_shader_binding::gpu;

use crate::global_descriptor_sets::{GlobalDescriptorSets, StaticDescriptorBinding};

// Sampler 管理器
pub struct RenderSamplerManager {
    _samplers: [GfxSampler; gpu::ESamplerType__Count_ as usize],
}

impl RenderSamplerManager {
    pub fn new(ctx: GfxDeviceCtx<'_>, render_descriptor_sets: &GlobalDescriptorSets) -> Self {
        let samplers = Self::create_sampler(ctx);

        // sampler 写入 descriptor set
        let write_sampler = StaticDescriptorBinding::samplers().write_image(
            render_descriptor_sets.sampler_set().handle(),
            0,
            samplers.iter().map(|samlper| vk::DescriptorImageInfo::default().sampler(samlper.handle())).collect_vec(),
        );
        ctx.device().write_descriptor_sets(std::slice::from_ref(&write_sampler));

        Self { _samplers: samplers }
    }

    fn create_sampler(ctx: GfxDeviceCtx<'_>) -> [GfxSampler; gpu::ESamplerType__Count_ as usize] {
        let mut sampler_descs =
            [0; gpu::ESamplerType__Count_ as usize].map(|_| (String::new(), GfxSamplerDesc::default()));

        fn create_sampler_desc(filter: vk::Filter, address_mode: vk::SamplerAddressMode) -> GfxSamplerDesc {
            GfxSamplerDesc {
                mag_filter: filter,
                min_filter: filter,
                mipmap_mode: if filter == vk::Filter::LINEAR {
                    vk::SamplerMipmapMode::LINEAR
                } else {
                    vk::SamplerMipmapMode::NEAREST
                },
                address_mode_u: address_mode,
                address_mode_v: address_mode,
                address_mode_w: address_mode,
                ..Default::default()
            }
        }

        sampler_descs[gpu::ESamplerType_PointRepeat as usize] =
            ("PointRepeat".to_string(), create_sampler_desc(vk::Filter::NEAREST, vk::SamplerAddressMode::REPEAT));
        sampler_descs[gpu::ESamplerType_PointClamp as usize] =
            ("PointClamp".to_string(), create_sampler_desc(vk::Filter::NEAREST, vk::SamplerAddressMode::CLAMP_TO_EDGE));
        sampler_descs[gpu::ESamplerType_LinearRepeat as usize] =
            ("LinearRepeat".to_string(), create_sampler_desc(vk::Filter::LINEAR, vk::SamplerAddressMode::REPEAT));
        sampler_descs[gpu::ESamplerType_LinearClamp as usize] =
            ("LinearClamp".to_string(), create_sampler_desc(vk::Filter::LINEAR, vk::SamplerAddressMode::CLAMP_TO_EDGE));
        sampler_descs[gpu::ESamplerType_AnisoRepeat as usize] = (
            "AnisoRepeat".to_string(),
            GfxSamplerDesc {
                max_anisotropy: 16,
                ..create_sampler_desc(vk::Filter::LINEAR, vk::SamplerAddressMode::REPEAT)
            },
        );
        sampler_descs[gpu::ESamplerType_AnisoClamp as usize] = (
            "AnisoClamp".to_string(),
            GfxSamplerDesc {
                max_anisotropy: 16,
                ..create_sampler_desc(vk::Filter::LINEAR, vk::SamplerAddressMode::CLAMP_TO_EDGE)
            },
        );

        sampler_descs.map(|(name, desc)| GfxSampler::new(ctx, &desc, format!("bindless-sampler-{}", name)))
    }
}

// 销毁
impl RenderSamplerManager {
    pub fn destroy(self, ctx: GfxDeviceCtx<'_>) {
        let Self { _samplers } = self;
        for sampler in _samplers {
            sampler.destroy(ctx);
        }
    }
}
