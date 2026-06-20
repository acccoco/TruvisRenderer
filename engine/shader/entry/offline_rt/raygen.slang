/// @file raygen.slang
/// @brief offline RT raygen 入口编排。
///
/// 离线 raygen 只负责生成当前 sample 的 HDR path tracing 结果。它复用 realtime RT 的
/// 材质、NEE、MIS 和 path state helper，但不写 GBuffer、DLSS input 或 ReSTIR reservoir。

#include "./payload.slangi"

#include "lib/core/random.slangi"
#include "lib/realtime_rt/raygen_debug_state.slangi"
#include "lib/realtime_rt/raygen_direct_lighting.slangi"
#include "lib/realtime_rt/raygen_material.slangi"
#include "lib/realtime_rt/raygen_path_state.slangi"

static RayDesc offline_camera_ray(uint2 thread_id)
{
    // 离线 primary ray jitter 来自 `offline_rt::PushConstants`，单位是 pixel。它不读取
    // `per_frame_data.temporal_jitter_px`，因此 DLSS 开关或 realtime frame state 不会改变 reference 序列。
    const float2 pixel_center = float2(thread_id) + float2(0.5f, 0.5f) + push_const.sample_jitter_px;
    const float2 in_uv = pixel_center / float2(DispatchRaysDimensions().xy);
    const float4 target_in_view = mul(
        per_frame_data.inv_projection,
        float4(in_uv.x * 2.0 - 1.0, 1.0 - in_uv.y * 2.0, 1.0, 1.0)
    );
    const float4 direction = mul(per_frame_data.inv_view, float4(normalize(target_in_view.xyz), 0.0));

    RayDesc ray;
    ray.Origin = per_frame_data.camera_pos;
    ray.Direction = direction.xyz;
    ray.TMin = 0.001f;
    ray.TMax = 10000.0f;
    return ray;
}

[shader("raygeneration")]
void main_ray_gen()
{
    const uint2 thread_id = DispatchRaysIndex().xy;
    // 随机种子只由像素坐标和离线 spp_idx 决定。reset 后同样的 sample index 会复现同样序列，
    // 而不是跟随 realtime frame_id 或 DLSS jitter 漂移。
    const uint random_seed = Random::tea(
        thread_id.y * DispatchRaysDimensions().x + thread_id.x,
        push_const.spp_idx
    );

    RayDesc ray = offline_camera_ray(thread_id);
    RtTracePayload payload = RtTracePayload::make(random_seed);
    RtPathState path = RtPathState::make();
    RtDebugAccumulator debug_state = RtDebugAccumulator::make();

    for (uint depth = 0; depth < max_depth; ++depth)
    {
        TraceRay(
            offline_rt::rt_tlas,
            RAY_FLAG_NONE,
            0xFF,
            0,
            0,
            0,
            ray,
            payload
        );

        RtSurfaceHit surface = payload.surface;

        float3 debug_color;
        if (RtDebugAccumulator::try_surface_channel(surface, push_const.channel, debug_color))
        {
            path.radiance = debug_color;
            break;
        }

        if (!payload.hit)
        {
            const float3 sky_color = payload.surface.emissive;
            const float3 sky_contribution = path.add_sky_miss(depth, sky_color, ray.Direction);
            debug_state.record_hdri(sky_contribution);
            break;
        }

        if (surface.material_type == MaterialType::EMISSIVE)
        {
            const float3 emissive_contribution = path.add_emissive(depth, surface, ray.Direction);
            debug_state.record_emissive(emissive_contribution);
            break;
        }

        const RtDirectLightingSample direct_lighting = RtDirectLighting::sample_unified(
            payload,
            surface,
            -ray.Direction,
            path.throughput
        );
        if (direct_lighting.valid)
        {
            path.add_radiance(direct_lighting.contribution);
            if (direct_lighting.light_kind == RtLightCandidate::kind_hdri())
            {
                debug_state.record_nee_hdri(depth, direct_lighting.contribution);
            }
            else if (direct_lighting.light_kind == RtLightCandidate::kind_emissive_triangle())
            {
                debug_state.record_nee_emissive(depth, direct_lighting.contribution);
            }
            else if (RtLightCandidate::is_analytic_kind(direct_lighting.light_kind))
            {
                debug_state.record_nee_analytic(depth, direct_lighting.contribution);
            }
        }

        const RtMaterialSample material_sample = RtMaterialSampler::sample_next(payload, surface, ray.Direction);
        path.apply_material_sample(surface, material_sample);
        if (material_sample.done)
        {
            break;
        }

        RtRussianRouletteResult rr_result = path.apply_russian_roulette(payload, depth);
        if (!rr_result.survived)
        {
            break;
        }

        ray.Origin = material_sample.origin;
        ray.Direction = material_sample.direction;
    }

    const float3 output_radiance = debug_state.resolve(push_const.channel, path.radiance);
    // raygen 只写当前 sample 的单帧 HDR 结果；跨帧 running average 由后续 accum compute pass 负责。
    offline_rt::rt_single_frame_output.Store(thread_id, float4(output_radiance, 1.0f));
}
