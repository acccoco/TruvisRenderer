/// @file rt_raygen.slang
/// @brief 光线生成着色器
///
/// 路径追踪主循环，协调以下模块：
/// - material_dispatch: 材质类型分发与 BRDF 采样
/// - nee: 下一事件估计（直接光照）
/// - debug_channel: 调试通道可视化
/// - gbuffer: GBuffer 输出
/// - irradiance_cache: Irradiance Cache 查询/更新

#include "./payload.slangi"

#include "lib/bindless_op.slangi"
#include "lib/env_map.slangi"
#include "lib/gbuffer.slangi"
#include "lib/mis.slangi"
#include "lib/sample/random.slangi"
#include "lib/irradiance_cache.slangi"
#include "lib/material_dispatch.slangi"
#include "lib/nee.slangi"
#include "lib/debug_channel.slangi"

// 开启后，虽然 active threads 的 coherence 指标从 50% 上升到了 70%
// SER 主要用于提升后续新的 shader invocation 的一致性
// 当前的架构重 ray-gen，轻 hit/miss shader，SER 带来的提升有限
#define USE_HIT_OBJECT_SER 0

// ============================================================================
// 相机和初始化
// ============================================================================

/// 生成相机光线
/// @param thread_id 当前线程 ID
/// @param subpixel_jitter 子像素抖动偏移
/// @return 光线描述
RayDesc generate_camera_ray(uint2 thread_id, float2 subpixel_jitter)
{
    const float2 pixel_center = float2(thread_id) + subpixel_jitter;
    const float2 in_uv = pixel_center / float2(DispatchRaysDimensions().xy);
    const float4 target_in_view = mul(per_frame_data.inv_projection, float4(in_uv.x * 2.0 - 1.0, 1.0 - in_uv.y * 2.0, 1.0, 1.0));
    const float4 direction = mul(per_frame_data.inv_view, float4(normalize(target_in_view.xyz), 0.0));

    RayDesc ray;
    ray.Origin = per_frame_data.camera_pos;
    ray.Direction = direction.xyz;
    ray.TMin = 0.001f;
    ray.TMax = 10000.0f;
    return ray;
}

/// 初始化光线载荷
/// @param random_seed 随机种子
/// @return 初始化的载荷
HitPayload init_payload(uint random_seed)
{
    HitPayload payload = {};
    payload.hit = false;
    payload.random_seed = random_seed;
    return payload;
}

// ============================================================================
// 辅助函数
// ============================================================================

/// 计算 SER coherence hint
uint compute_coherence_hint(const HitInfo info)
{
    return uint(info.material_type);
}

/// 计算 luminance
float luminance(float3 color)
{
    return dot(color, float3(0.2126f, 0.7152f, 0.0722f));
}

// ============================================================================
// Irradiance Cache 追踪状态
// ============================================================================

/// IC 追踪器：封装 Irradiance Cache 在路径追踪中的状态管理
struct IcTracker
{
    uint diffuse_bounce_count;  ///< Diffuse 弹射计数
    bool pending_update;        ///< 是否有待处理的 IC 更新操作
    bool pending_insert;        ///< 是否有待处理的 IC 插入操作
    bool has_query;             ///< 是否已进行 IC 查询
    uint entry_index;           ///< 待更新的 IC entry 索引
    float3 position;            ///< IC 记录点的世界位置
    float3 normal;              ///< IC 记录点的法线
    float3 ic_throughput;       ///< 从 IC 记录点开始的相对 throughput
    float3 ic_radiance;         ///< 累积的 outgoing radiance

    /// 创建初始状态
    static IcTracker create()
    {
        IcTracker t;
        t.diffuse_bounce_count = 0;
        t.pending_update = false;
        t.pending_insert = false;
        t.has_query = false;
        t.entry_index = 0xFFFFFFFF;
        t.position = float3(0.f);
        t.normal = float3(0.f);
        t.ic_throughput = float3(1.f);
        t.ic_radiance = float3(0.f);
        return t;
    }

    /// 是否有待处理的 IC 操作
    bool has_pending() { return pending_update || pending_insert; }

    /// 累积 radiance 贡献（用于 miss/emissive/NEE）
    [mutating]
    void accumulate_radiance(float3 contrib)
    {
        if (has_pending())
        {
            ic_radiance += contrib;
        }
    }

    /// 更新 throughput（BRDF 采样后调用）
    [mutating]
    void update_throughput(float3 mat_throughput)
    {
        if (has_pending())
        {
            ic_throughput *= mat_throughput;
        }
    }

    /// 俄罗斯轮盘赌缩放
    [mutating]
    void scale_by_rr(float p)
    {
        ic_throughput /= p;
    }

    /// 记录 diffuse 弹射
    [mutating]
    void on_diffuse_bounce()
    {
        diffuse_bounce_count++;
    }

    /// 尝试 IC 查询，返回是否应终止路径
    [mutating]
    bool try_query(
        const HitInfo info,
        uint ic_enabled,
        uint spp_idx,
        float3 throughput,
        inout float3 radiance,
        inout uint random_seed
    )
    {
        if (ic_enabled == 0) return false;
        if (diffuse_bounce_count < rt::IC_MIN_DIFFUSE_BOUNCES) return false;
        if (!ic::is_diffuse_surface(info.roughness, info.metallic)) return false;
        if (pending_update || pending_insert || has_query) return false;

        float3 ic_query_radiance;
        float ic_weight;
        uint ic_max_weight_entry_index;
        float ic_max_weight;

        bool ic_found = ic::fn::lookup(
            push_const.ic_table,
            push_const.ic_entry_pool,
            info.position,
            info.forward_normal,
            ic_query_radiance,
            ic_weight,
            ic_max_weight_entry_index,
            ic_max_weight
        );
        has_query = true;

        if (ic_found && ic_max_weight > ic::IC_LOOKUP_THRESHOLD)
        {
            // 概率性使用 IC 结果
            if (Random::rnd(random_seed) < 0.7f)
            {
                radiance += throughput * ic_query_radiance;
                return true; // 终止路径
            }
        }

        if (ic_found && ic_max_weight > ic::IC_UPDATE_THRESHOLD)
        {
            pending_update = true;
            entry_index = ic_max_weight_entry_index;
            position = info.position;
            normal = info.forward_normal;
            ic_throughput = float3(1.f);
            ic_radiance = float3(0.f);
        }
        else if (!ic_found && spp_idx == 0)
        {
            pending_insert = true;
            position = info.position;
            normal = info.forward_normal;
            ic_throughput = float3(1.f);
            ic_radiance = float3(0.f);
        }

        return false;
    }

    /// 路径追踪结束后执行 IC 更新/插入
    void finalize(uint ic_enabled, uint spp_idx)
    {
        if (ic_enabled == 0 || !has_pending()) return;

        if (pending_update)
        {
            ic::fn::update(
                push_const.ic_entry_pool,
                entry_index,
                position,
                normal,
                ic_radiance
            );
        }
        else if (pending_insert && spp_idx == 0)
        {
            ic::fn::insert(
                push_const.ic_table,
                push_const.ic_entry_pool,
                position,
                normal,
                ic_radiance
            );
        }
    }
};

// ============================================================================
// 主函数
// ============================================================================

[shader("raygeneration")]
void main_ray_gen()
{
    const uint accum_samples = per_frame_data.accum_frames * push_const.spp + push_const.spp_idx;
    if (accum_samples >= max_accum_samples)
    {
        return;
    }

    uint2 thread_id = DispatchRaysIndex().xy;

    // 子像素抖动
    uint random_seed = Random::tea(
        thread_id.y * DispatchRaysDimensions().x + thread_id.x,
        (uint)per_frame_data.frame_id + push_const.spp_idx
    );
    const float2 subpixel_jitter = (accum_samples == 0 || push_const.channel != 0) ? float2(0.5f) : float2(0.5f) + 0.375f * Random::rand_gaussian(random_seed);

    RayDesc ray = generate_camera_ray(thread_id, subpixel_jitter);
    HitPayload payload = init_payload(random_seed);

    // 路径追踪状态
    float3 radiance = float3(0.f);
    float3 throughput = float3(1.f);
    float prev_brdf_pdf = 1.f;
    bool has_diffuse = false;
    bool prev_is_delta = true; // 首次弹射视为 delta

    // 调试累积器
    debug_channel::DebugAccumulator debug_acc = debug_channel::DebugAccumulator::create();

    // 调试通道 9：首次命中位置的 IC 查询
    float3 first_hit_position = float3(0.f);
    float3 first_hit_normal = float3(0.f);
    bool first_hit_recorded = false;

    // GBuffer 与 IC 状态
    bool gbuffer_written = false;
    IcTracker ic_tracker = IcTracker::create();

    // ================================================================
    // 路径追踪主循环
    // ================================================================
    for (uint depth = 0; depth < max_depth; ++depth)
    {
        TraceRay(
            rt::rt_tlas,
            RAY_FLAG_NONE,
            0xFF, // cull mask
            0,    // ray type
            0,    // hit group index
            0,    // miss shader index
            ray, payload
        );

        // 调试通道（通道 1/2：首次命中即返回）
        float3 debug_color;
        if (debug_channel::handle_first_hit_debug(payload.info, push_const.channel, debug_color))
        {
            radiance = debug_color;
            break;
        }

        // IC 追踪模式：强制 diffuse 材质以提高缓存一致性
        if (payload.hit &&
            push_const.ic_enabled != 0 &&
            ic_tracker.diffuse_bounce_count >= rt::IC_MIN_DIFFUSE_BOUNCES &&
            ic::is_diffuse_surface(payload.info.roughness, payload.info.metallic))
        {
            payload.info.roughness = 1.0f;
            payload.info.metallic = 0.0f;
            payload.info.material_type = MaterialType::DIFFUSE;
        }

        // ================================================================
        // 处理 miss（命中 HDRI）
        // ================================================================
        if (!payload.hit)
        {
            const float3 sky_color = payload.info.emissive;

            if (!gbuffer_written)
            {
                gbuffer::write_gbuffer_miss(thread_id);
                gbuffer_written = true;
            }

            if (prev_is_delta || depth == 0)
            {
                const float3 contrib = sky_color * throughput;
                radiance += contrib;
                debug_acc.accumulate_hdri(contrib);
                ic_tracker.accumulate_radiance(sky_color * ic_tracker.ic_throughput);
            }
            else
            {
                const float env_pdf = env_map_uniform_pdf();
                const float mis_weight = MIS::power_heuristic(prev_brdf_pdf, env_pdf);
                const float3 contrib = sky_color * throughput * mis_weight;
                radiance += contrib;
                debug_acc.accumulate_hdri(contrib);
                ic_tracker.accumulate_radiance(sky_color * ic_tracker.ic_throughput * mis_weight);
            }
            break;
        }

        // 记录首次命中位置（用于调试通道 9）
        if (!first_hit_recorded)
        {
            first_hit_position = payload.info.position;
            first_hit_normal = payload.info.forward_normal;
            first_hit_recorded = true;
        }

        // ================================================================
        // GBuffer 输出（仅首次命中时写入）
        // ================================================================
        if (!gbuffer_written)
        {
            gbuffer::GBufferData gbuffer_data = gbuffer::make_gbuffer_data(
                payload.info.position,
                payload.info.forward_normal,
                payload.info.hit_t,
                payload.info.base_color,
                payload.info.metallic,
                payload.info.roughness
            );
            gbuffer::write_gbuffer_hit(thread_id, gbuffer_data);
            gbuffer_written = true;
        }

        // ================================================================
        // 处理自发光物体
        // ================================================================
        if (payload.info.material_type == MaterialType::EMISSIVE)
        {
            const float3 contrib = payload.info.emissive * throughput;
            radiance += contrib;
            debug_acc.accumulate_emissive(contrib);
            ic_tracker.accumulate_radiance(payload.info.emissive * ic_tracker.ic_throughput);
            break;
        }

        // ================================================================
        // NEE: 下一事件估计
        // ================================================================
        nee::NeeResult nee_result = nee::compute_nee(
            payload.info, ray.Direction, throughput,
            rt::rt_tlas, gpu_scene, payload.random_seed
        );
        if (nee_result.valid)
        {
            radiance += nee_result.contribution;
            debug_acc.accumulate_nee(nee_result.contribution, depth);
            // IC 追踪：按 ic_throughput/throughput 比例缩放 NEE 贡献
            if (ic_tracker.has_pending())
            {
                const float3 ic_nee_contrib = nee_result.contribution * ic_tracker.ic_throughput / max(throughput, float3(1e-7f));
                ic_tracker.accumulate_radiance(ic_nee_contrib);
            }
        }

        // ================================================================
        // Irradiance Cache 查询
        // ================================================================
        if (ic_tracker.try_query(
            payload.info, push_const.ic_enabled, push_const.spp_idx,
            throughput, radiance, payload.random_seed))
        {
            break; // IC 查询成功，终止路径
        }

        // ================================================================
        // 材质处理：BRDF 采样，计算下一条光线
        // ================================================================
        float3 next_origin, next_dir, mat_throughput, emission;
        bool done;
        float brdf_pdf;
        material_dispatch::process_material(
            payload.info,
            ray.Direction,
            payload.random_seed,
            next_origin,
            next_dir,
            mat_throughput,
            emission,
            done,
            brdf_pdf
        );

        // 累积自发光
        radiance += emission * throughput;

        if (done)
        {
            break;
        }

        // 更新 throughput
        throughput *= mat_throughput;
        ic_tracker.update_throughput(mat_throughput);

        // 记录路径信息供下次 MIS 使用
        prev_brdf_pdf = brdf_pdf;
        prev_is_delta = payload.info.is_delta_path();

        // 检测 diffuse 弹射
        if (ic::is_diffuse_surface(payload.info.roughness, payload.info.metallic))
        {
            has_diffuse = true;
            ic_tracker.on_diffuse_bounce();
        }

        // 俄罗斯轮盘赌
        if (depth >= 3)
        {
            float p = max(throughput.x, max(throughput.y, throughput.z));
            p = clamp(p, 0.05f, 0.95f);
            if (Random::rnd(payload.random_seed) > p)
            {
                break;
            }
            throughput /= p;
            ic_tracker.scale_by_rr(p);
        }

        // 设置下一条光线
        ray.Origin = next_origin;
        ray.Direction = next_dir;
    }

    // ================================================================
    // IC 更新/插入（路径追踪结束后）
    // ================================================================
    ic_tracker.finalize(push_const.ic_enabled, push_const.spp_idx);

    // ================================================================
    // 选择调试通道输出
    // ================================================================
    float3 output_radiance = debug_channel::select_output(push_const.channel, radiance, debug_acc);

    // 通道 9：首次命中位置的 IC 查询
    if (push_const.channel == 9)
    {
        if (first_hit_recorded)
        {
            float3 ic_query_radiance;
            float ic_weight;
            uint ic_max_weight_entry_index;
            float ic_max_weight;

            bool ic_found = ic::fn::lookup(
                push_const.ic_table,
                push_const.ic_entry_pool,
                first_hit_position,
                first_hit_normal,
                ic_query_radiance,
                ic_weight,
                ic_max_weight_entry_index,
                ic_max_weight
            );

            output_radiance = ic_found ? ic_query_radiance : float3(0.f);
        }
        else
        {
            output_radiance = float3(0.f);
        }
    }

    // 输出单帧结果（累积逻辑移至单独的 accum pass）
    rt::rt_single_frame_output.Store(thread_id, float4(output_radiance, 1.f));
}

