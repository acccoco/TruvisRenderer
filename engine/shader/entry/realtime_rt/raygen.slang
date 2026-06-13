/// @file raygen.slang
/// @brief realtime RT raygen 入口编排。
///
/// raygen 只负责每像素初始化、TraceRay path loop 顺序和最终写出。材质采样、直接光照、
/// DLSS/GBuffer 输入、debug 累积和 IC 实验路径分别由 `lib/realtime_rt/raygen_*.slangi`
/// 中的明确职责类型承载。

#include "./payload.slangi"

#include "lib/core/random.slangi"
#include "lib/realtime_rt/raygen_debug_state.slangi"
#include "lib/realtime_rt/raygen_direct_lighting.slangi"
#include "lib/realtime_rt/raygen_ic_state.slangi"
#include "lib/realtime_rt/raygen_material.slangi"
#include "lib/realtime_rt/raygen_path_state.slangi"
#include "lib/realtime_rt/raygen_view.slangi"

// 开启后，虽然 active threads 的 coherence 指标从 50% 上升到了 70%
// SER 主要用于提升后续新的 shader invocation 的一致性
// 当前仍保持 raygen 编排为主，暂不为了职责拆分引入 callable / SER 相关管线复杂度。
#define USE_HIT_OBJECT_SER 0

[shader("raygeneration")]
void main_ray_gen()
{
    const uint2 thread_id = DispatchRaysIndex().xy;
    const uint random_seed = Random::tea(
        thread_id.y * DispatchRaysDimensions().x + thread_id.x,
        (uint)per_frame_data.frame_id + push_const.spp_idx
    );

    RayDesc ray = RtRaygenView::camera_ray(thread_id);
    RtTracePayload payload = RtTracePayload::make(random_seed);
    RtPathState path = RtPathState::make();
    RtDebugAccumulator debug_state = RtDebugAccumulator::make();
    RtIrradianceCacheState ic_state = RtIrradianceCacheState::make();

    bool gbuffer_written = false;

    for (uint depth = 0; depth < max_depth; ++depth)
    {
        TraceRay(
            realtime_rt::rt_tlas,
            RAY_FLAG_NONE,
            0xFF,
            0,
            0,
            0,
            ray,
            payload
        );

        RtSurfaceHit surface = payload.surface;

        // surface-only debug 通道沿用旧语义：miss shader 填入的虚拟 surface 也参与可视化。
        float3 debug_color;
        if (RtDebugAccumulator::try_surface_channel(surface, push_const.channel, debug_color))
        {
            path.radiance = debug_color;
            break;
        }

        if (!payload.hit)
        {
            if (!gbuffer_written)
            {
                RtRaygenView::write_primary_miss_outputs(thread_id);
                gbuffer_written = true;
            }

            const float3 sky_color = payload.surface.emissive;
            const float3 sky_contribution = path.add_sky_miss(depth, sky_color);
            debug_state.record_hdri(sky_contribution);
            ic_state.record_sky_miss(depth, sky_color, path);
            break;
        }

        ic_state.force_diffuse_surface(surface);

        debug_state.record_first_hit(surface);

        if (!gbuffer_written)
        {
            RtRaygenView::write_primary_hit_outputs(thread_id, surface, ray.Direction);
            gbuffer_written = true;
        }

        if (surface.material_type == MaterialType::EMISSIVE)
        {
            const float3 emissive_contribution = path.add_emissive(surface);
            debug_state.record_emissive(emissive_contribution);
            ic_state.record_emissive(surface);
            break;
        }

        const RtDirectLightingSample direct_lighting = RtDirectLighting::sample_hdri(
            payload,
            surface,
            -ray.Direction,
            path.throughput
        );
        if (direct_lighting.valid)
        {
            path.add_radiance(direct_lighting.contribution);
            debug_state.record_nee(depth, direct_lighting.contribution);
            ic_state.record_direct_lighting(direct_lighting);
        }

        if (ic_state.try_resolve_or_schedule(payload, surface, path))
        {
            break;
        }

        const RtMaterialSample material_sample = RtMaterialSampler::sample_next(payload, surface, ray.Direction);
        path.apply_material_sample(surface, material_sample);
        if (material_sample.done)
        {
            break;
        }
        ic_state.apply_material_sample(material_sample);
        ic_state.note_surface_after_sample(surface);

        RtRussianRouletteResult rr_result = path.apply_russian_roulette(payload, depth);
        if (!rr_result.survived)
        {
            break;
        }
        ic_state.scale_throughput(rr_result.throughput_scale);

        ray.Origin = material_sample.origin;
        ray.Direction = material_sample.direction;
    }

    ic_state.flush();

    const float3 output_radiance = debug_state.resolve(push_const.channel, path.radiance);
    realtime_rt::rt_single_frame_output.Store(thread_id, float4(output_radiance, 1.0f));
}
