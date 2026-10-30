/// @file raygen.slang
/// @brief realtime RT raygen 入口编排。
///
/// raygen 只负责每像素初始化、TraceRay path loop 顺序和最终写出。材质采样、直接光照、
/// DLSS/GBuffer 输入和 debug 累积分别由 `lib/realtime_rt/raygen_*.slangi` 中的明确职责类型承载。

#include "lib/renderer/realtime_rt/payload.slangi"

#include "lib/engine/core/random.slangi"
#include "lib/renderer/realtime_rt/raygen_debug_state.slangi"
#include "lib/renderer/realtime_rt/raygen_direct_lighting.slangi"
#include "lib/renderer/realtime_rt/raygen_material.slangi"
#include "lib/renderer/realtime_rt/raygen_path_state.slangi"
#include "lib/renderer/realtime_rt/raygen_view.slangi"
#include "lib/renderer/realtime_rt/restir_di.slangi"
#include "lib/renderer/realtime_rt/sharc_integration.slangi"

// 开启后，虽然 active threads 的 coherence 指标从 50% 上升到了 70%
// SER 主要用于提升后续新的 shader invocation 的一致性
// 当前仍保持 raygen 编排为主，暂不为了职责拆分引入 callable / SER 相关管线复杂度。
#define USE_HIT_OBJECT_SER 0

[shader("raygeneration")]
void main_ray_gen()
{
    const uint2 thread_id = DispatchRaysIndex().xy;

    /// SHARC 维护阶段复用同一 RT pipeline，在主路径前依次 Update / Snapshot / Resolve。
    if (push_const.sharc_phase == renderer::render_passes::realtime_rt::SHARC_PHASE_UPDATE)
    {
        RtSharc::run_update(thread_id);
        return;
    }
    if (push_const.sharc_phase == renderer::render_passes::realtime_rt::SHARC_PHASE_SNAPSHOT)
    {
        RtSharc::run_snapshot(thread_id);
        return;
    }
    if (push_const.sharc_phase == renderer::render_passes::realtime_rt::SHARC_PHASE_RESOLVE)
    {
        RtSharc::run_resolve(thread_id);
        return;
    }

    if (push_const.restir_di_phase != renderer::render_passes::realtime_rt::RESTIR_DI_PHASE_PATH)
    {
        // temporal/spatial/final phase 是 image-space pass，不应再次发 camera ray 或推进 path state。
        // path phase 已经写入 HDR/GBuffer/motion/surface key，后续 phase 只消费这些结果。
        RtRestirDI::run_non_path_phase(thread_id);
        return;
    }

    const uint random_seed = Random::tea(
        thread_id.y * DispatchRaysDimensions().x + thread_id.x,
        (uint)engine::binding::per_frame_data.frame_id + push_const.spp_idx
    );

    RayDesc ray = RtRaygenView::camera_ray(thread_id);
    RtTracePayload payload = RtTracePayload::make(random_seed);
    RtPathState path = RtPathState::make();
    RtDebugAccumulator debug_state = RtDebugAccumulator::make();

    // GBuffer / DLSS 输入只描述 camera primary 第一次可见事件；后续反弹不能覆盖它。
    bool gbuffer_written = false;

    for (uint depth = 0; depth < max_depth; ++depth)
    {
        const bool has_bsdf_continuation = depth + 1u < max_depth;
        // 每次 TraceRay 只返回一个 surface/miss 事件；path 积分和 debug 状态都在 raygen 侧推进。
        TraceRay(
            renderer::render_passes::realtime_rt::rt_tlas,
            RAY_FLAG_NONE,
            0xFF,
            0,
            0,
            0,
            ray,
            payload
        );

        RtSurfaceHit surface = payload.surface;
        const float specular_hit_distance = depth == 0 && payload.hit
            ? RtRaygenView::primary_specular_hit_distance(surface, ray.Direction)
            : renderer::render_passes::realtime_rt::RR_FAR_HIT_DISTANCE;

        // surface-only debug 通道沿用旧语义：miss shader 填入的虚拟 surface 也参与可视化。
        float3 debug_color;
        bool surface_debug_done = RtDebugAccumulator::try_surface_channel(surface, push_const.channel, debug_color);
        if (!surface_debug_done && push_const.channel == RT_DEBUG_SPECULAR_HIT_DISTANCE)
        {
            // sentinel 为紫色，有效距离按 d/(1+d) 映射灰度；复用本像素查询，不重复 trace。
            debug_color = specular_hit_distance == renderer::render_passes::realtime_rt::RR_FAR_HIT_DISTANCE
                ? float3(1.0f, 0.0f, 1.0f)
                : float3(specular_hit_distance / (1.0f + specular_hit_distance));
            surface_debug_done = true;
        }
        if (surface_debug_done)
        {
            if (!gbuffer_written && depth == 0)
            {
                if (payload.hit)
                {
                    RtRaygenView::write_primary_hit_outputs(thread_id, surface, ray.Direction, specular_hit_distance);
                    if (RtRestirDI::enabled())
                    {
                        // surface-only debug 也要写 ReSTIR surface key/empty reservoir，保证后续
                        // final/debug phase 看到的是本帧有效状态，而不是 storage image 中的旧内容。
                        RtRestirDI::store_primary_surface_key(thread_id, surface);
                        RtRestirDI::store_initial(thread_id, RtReservoir::empty());
                    }
                }
                else
                {
                    RtRaygenView::write_primary_miss_outputs(thread_id, ray.Direction);
                    if (RtRestirDI::enabled())
                    {
                        // miss/debug 像素不属于 primary direct lighting，必须显式写空 key 和空 reservoir。
                        RtRestirDI::store_surface_key(thread_id, RtPrimarySurfaceKey::empty());
                        RtRestirDI::store_initial(thread_id, RtReservoir::empty());
                    }
                }
                gbuffer_written = true;
            }

            path.radiance = debug_color;
            break;
        }

        if (!payload.hit)
        {
            if (!gbuffer_written)
            {
                // sky pixel 也必须写满 GBuffer/DLSS 输入，避免 storage image 中保留上一帧几何数据。
                RtRaygenView::write_primary_miss_outputs(thread_id, ray.Direction);
                if (RtRestirDI::enabled())
                {
                    // sky miss 没有 primary surface；清空 ReSTIR 输入，防止 temporal/final phase
                    // 在背景像素上复用上一帧几何 reservoir。
                    RtRestirDI::store_surface_key(thread_id, RtPrimarySurfaceKey::empty());
                    RtRestirDI::store_initial(thread_id, RtReservoir::empty());
                }
                gbuffer_written = true;
            }

            /// 两个表面 cache 通道在 primary miss 时没有观测对象，不能把 sky radiance 冒充缓存。
            /// 上方仍刷新 GBuffer / DLSS / ReSTIR 输入，只有可视化结果提前结束。
            if (depth == 0 && (push_const.channel == RT_DEBUG_SHARC_HASH_GRID || push_const.channel == RT_DEBUG_SHARC_CACHE))
            {
                break;
            }

            const float3 sky_color = payload.surface.emissive;
            const float3 sky_contribution = path.add_sky_miss(depth, sky_color, ray.Direction);
            debug_state.record_hdri(sky_contribution);
            break;
        }

        if (!gbuffer_written)
        {
            // 首次命中时同步写 GBuffer、DLSS depth/motion 和 RR 材质输入。
            RtRaygenView::write_primary_hit_outputs(thread_id, surface, ray.Direction, specular_hit_distance);
            if (RtRestirDI::enabled())
            {
                RtRestirDI::store_primary_surface_key(thread_id, surface);
            }
            gbuffer_written = true;
        }

        /// SHARC debug 在 primary hit 提前退出，不生成本帧候选。
        /// surface 已刷新，initial 也必须清空，不能把本 FIF 槽的旧候选配到当前表面。
        if (depth == 0 && (push_const.channel == RT_DEBUG_SHARC_HASH_GRID || push_const.channel == RT_DEBUG_SHARC_CACHE))
        {
            if (RtRestirDI::enabled())
            {
                RtRestirDI::store_initial(thread_id, RtReservoir::empty());
            }
            path.radiance = push_const.channel == RT_DEBUG_SHARC_HASH_GRID
                ? RtSharc::debug_hash_grid_color(surface)
                : RtSharc::debug_cache_radiance(surface);
            break;
        }

        if (any(surface.emissive > 0.0))
        {
            const float3 emission = path.add_emissive(depth, surface);
            debug_state.record_emissive(emission);
        }

        if (surface.material_type == MaterialType::EMISSIVE)
        {
            if (RtRestirDI::enabled() && depth == 0)
            {
                // 直接可见自发光 surface 仍走 hit emission 旧路径，不属于 ReSTIR DI 的 primary direct NEE。
                // 写空 initial 能让 final phase 保持无贡献，避免 emission 被重复加入。
                RtRestirDI::store_initial(thread_id, RtReservoir::empty());
            }

            break;
        }


        // SHARC query（第九阶段）：非 primary 的合适 surface 命中世界空间缓存时，用 cached radiance
        // 估计该顶点之后的全部出射光并提前终止路径。primary / 当前 delta / 上一段 delta /
        // 段长不足 / 锥角过窄都不查询；其中上一段 delta 守卫用于保护镜面反射和玻璃透射后
        // 第一层可见 surface，避免 sharp specular/transmission view 被低频 voxel cache 替代。
        // cached radiance 已包含该顶点的直接光，所以命中后不再做 NEE / BSDF 续路，避免双计能量。
        if (RtSharc::should_query(surface, depth, path.prev_is_delta))
        {
            float3 cached_radiance;
            if (RtSharc::query_radiance(surface, cached_radiance))
            {
                const float3 sharc_contribution = cached_radiance * path.throughput;
                path.add_radiance(sharc_contribution);
                debug_state.record_sharc_query(depth, sharc_contribution);
                break;
            }
        }

        if (RtRestirDI::should_skip_primary_nee(depth, surface))
        {
            // primary 非 delta 非 emissive surface 的 direct NEE 由 ReSTIR final phase 统一追加。
            // 这里只生成 initial reservoir，避免同一条 primary direct light 被普通 NEE 和 ReSTIR 双算。
            RtRestirDI::store_initial(thread_id, RtRestirDI::make_initial_reservoir(payload, surface, -ray.Direction));
        }
        else
        {
            if (RtRestirDI::enabled() && depth == 0)
            {
                // ReSTIR 已开启但当前 primary 不符合替换条件时，也要写空 initial。
                // temporal/final phase 依赖这个空值判断该像素不参与 ReSTIR。
                RtRestirDI::store_initial(thread_id, RtReservoir::empty());
            }

            const RtDirectLightingSample direct_lighting = RtDirectLighting::sample_unified(
                payload,
                surface,
                -ray.Direction,
                path.throughput,
                has_bsdf_continuation
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
        }

        /// 末顶点已完成 emission/NEE，没有下一次 trace，不消耗材质或 roulette 随机槽。
        if (!has_bsdf_continuation)
        {
            break;
        }

        const RtMaterialSample material_sample = RtMaterialSampler::sample_next(payload, surface, ray.Direction);
        path.apply_material_sample(surface, material_sample);
        if (material_sample.done)
        {
            break;
        }

        if (!path.apply_russian_roulette(payload, depth))
        {
            break;
        }

        ray.Origin = material_sample.origin;
        ray.Direction = material_sample.direction;
    }

    const float3 output_radiance = debug_state.resolve(push_const.channel, path.radiance);
    renderer::render_passes::realtime_rt::rt_single_frame_output.Store(thread_id, float4(output_radiance, 1.0f));
}
