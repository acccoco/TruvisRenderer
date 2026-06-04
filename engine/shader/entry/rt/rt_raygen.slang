/// @file rt_raygen.slang
/// @brief 光线生成着色器（重构版）
///
/// 所有材质计算都在 ray gen 中进行：
/// - BRDF 采样和计算
/// - throughput 计算
/// - radiance 累积
/// - 俄罗斯轮盘赌终止

#include "./payload.slangi"

#include "lib/bindless_op.slangi"
#include "lib/env_map.slangi"
#include "lib/gbuffer.slangi"
#include "lib/mis.slangi"
#include "lib/pbr.slangi"
#include "lib/sample/random.slangi"
#include "lib/sample/sample.slangi"
#include "lib/irradiance_cache.slangi"

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

float compute_device_depth(float3 world_position)
{
    // DLSS SR 当前 tag 的是 kBufferTypeDepth，需要 projection 后的 device depth。
    // GBufferB.w 仍保存 ray hit_t，两者语义不要混用。
    const float4 view_position = mul(per_frame_data.view, float4(world_position, 1.0f));
    const float4 clip_position = mul(per_frame_data.projection, view_position);
    return saturate(clip_position.z / clip_position.w);
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
// BRDF 采样函数
// ============================================================================

/// 漫反射采样
/// @param info 命中信息
/// @param random_seed 随机种子
/// @param out_dir 输出：采样方向
/// @param out_throughput 输出：throughput = BRDF * cos / pdf
/// @param out_pdf 输出：采样 PDF
void sample_diffuse(
    const HitInfo info,
    inout uint random_seed,
    out float3 out_dir,
    out float3 out_throughput,
    out float out_pdf
)
{
    // 半球 cos 采样
    // BRDF = diffuse_color / pi
    // PDF = cos(theta) / pi
    // throughput = BRDF * cos(theta) / PDF = diffuse_color
    const float3 local_dir = Sample::get_cos_hemisphere_sample(random_seed);
    float3 tangent, bitangent;
    Sample::create_local_coord(info.forward_normal, tangent, bitangent);
    out_dir = normalize(local_dir.x * tangent + local_dir.y * bitangent + local_dir.z * info.forward_normal);

    const float3 diffuse_color = (1.f - info.metallic) * info.base_color;
    out_throughput = diffuse_color;
    out_pdf = DiffuseBrdf::pdf(info.forward_normal, out_dir);
}

/// 镜面反射采样（GGX）
/// @param info 命中信息
/// @param view_dir 观察方向（从命中点到相机）
/// @param random_seed 随机种子
/// @param out_dir 输出：采样方向
/// @param out_throughput 输出：throughput = BRDF * cos / pdf
/// @param out_pdf 输出：采样 PDF
void sample_specular(
    const HitInfo info,
    const float3 view_dir,
    inout uint random_seed,
    out float3 out_dir,
    out float3 out_throughput,
    out float out_pdf
)
{
    const float reflectance = 0.8f;
    const float3 f0 = lerp(0.16 * reflectance * reflectance, info.base_color, info.metallic);
    const float real_roughness = info.roughness * info.roughness;

    // 根据 NDF 采样半程向量 H
    float3 h = SpecularBrdf::ggx_sample(random_seed, real_roughness, info.forward_normal);
    SpecularBrdf specular_brdf = SpecularBrdf(f0, real_roughness, info.forward_normal, view_dir, h);

    out_throughput = specular_brdf.brdf_with_pdf() * specular_brdf.get_ndotl();
    out_dir = specular_brdf.light;
    out_pdf = specular_brdf.ggx_pdf();
}

/// 完美镜面反射（delta 路径）
/// @param info 命中信息
/// @param ray_dir 入射光线方向
/// @param out_dir 输出：反射方向
/// @param out_throughput 输出：throughput
void sample_perfect_reflection(
    const HitInfo info,
    const float3 ray_dir,
    out float3 out_dir,
    out float3 out_throughput
)
{
    out_dir = reflect(ray_dir, info.forward_normal);
    // 完美镜面反射的 throughput 基于菲涅尔
    const float reflectance = 0.8f;
    const float3 f0 = lerp(0.16 * reflectance * reflectance, info.base_color, info.metallic);
    const float cos_theta = saturate(dot(-ray_dir, info.forward_normal));
    const float fresnel = pow(1.f - cos_theta, 5.f);
    out_throughput = f0 + (1.f - f0) * fresnel;
}

/// 折射采样
/// @param info 命中信息
/// @param ray_dir 入射光线方向
/// @param out_dir 输出：折射方向
/// @param out_throughput 输出：throughput
/// @return true 表示折射成功，false 表示全内反射
bool sample_refraction(
    const HitInfo info,
    const float3 ray_dir,
    out float3 out_dir,
    out float3 out_throughput
)
{
    // 根据光线与原始法线的夹角判断是进入还是离开介质
    const float eta = dot(ray_dir, info.origin_normal) > 0.f ? info.ior : 1.f / info.ior;

    out_dir = refract(ray_dir, info.forward_normal, eta);
    if (length(out_dir) < 1e-6f)
    {
        out_dir = float3(0.f);
        out_throughput = float3(0.f);
        // 全内反射
        return false;
    }

    out_throughput = info.base_color;
    return true;
}

// ============================================================================
// 材质处理
// ============================================================================

/// 处理自发光材质（EMISSIVE）
/// @param info 命中信息
/// @param out_emission 输出：自发光
/// @param out_done 输出：是否终止
/// @return true 表示是自发光材质，已处理
bool handle_emissive(
    const HitInfo info,
    out float3 out_emission,
    out bool out_done
)
{
    out_emission = float3(0.f);
    out_done = false;

    if (info.material_type == MaterialType::EMISSIVE)
    {
        out_emission = info.emissive;
        out_done = true;
        return true;
    }

    return false;
}

/// 处理透明材质（TRANSPARENT）- Delta 路径
/// @param payload 光线载荷
/// @param info 命中信息
/// @param ray_dir 当前光线方向
/// @param out_origin 输出：下一条光线起点
/// @param out_dir 输出：下一条光线方向
/// @param out_throughput 输出：throughput
void handle_transparent(
    inout HitPayload payload,
    const HitInfo info,
    const float3 ray_dir,
    out float3 out_origin,
    out float3 out_dir,
    out float3 out_throughput
)
{
    // 折射 vs 反射的选择
    const bool choose_refract = Random::rnd(payload.random_seed) >= info.opaque;
    if (choose_refract)
    {
        if (!sample_refraction(info, ray_dir, out_dir, out_throughput))
        {
            // 全内反射
            sample_perfect_reflection(info, ray_dir, out_dir, out_throughput);
        }
    }
    else
    {
        // 反射
        sample_perfect_reflection(info, ray_dir, out_dir, out_throughput);
    }
    out_origin = info.position + 0.0001f * out_dir;
}

/// 处理镜面材质（SPECULAR）- Delta 路径
/// @param info 命中信息
/// @param ray_dir 当前光线方向
/// @param out_origin 输出：下一条光线起点
/// @param out_dir 输出：下一条光线方向
/// @param out_throughput 输出：throughput
void handle_specular(
    const HitInfo info,
    const float3 ray_dir,
    out float3 out_origin,
    out float3 out_dir,
    out float3 out_throughput
)
{
    // Delta 镜面反射
    sample_perfect_reflection(info, ray_dir, out_dir, out_throughput);
    out_origin = info.position + 0.0001f * out_dir;
}

/// 处理漫反射材质（DIFFUSE）
/// 随机选择 diffuse 或 NDF 采样策略，计算完整 BRDF = diffuse + specular
/// @param payload 光线载荷
/// @param info 命中信息
/// @param ray_dir 当前光线方向
/// @param out_origin 输出：下一条光线起点
/// @param out_dir 输出：下一条光线方向
/// @param out_throughput 输出：throughput = BRDF / PDF
/// @param out_brdf_pdf 输出：混合 BRDF 采样 PDF（用于 MIS）
void handle_diffuse(
    inout HitPayload payload,
    const HitInfo info,
    const float3 ray_dir,
    out float3 out_origin,
    out float3 out_dir,
    out float3 out_throughput,
    out float out_brdf_pdf
)
{
    const float3 view_dir = -ray_dir;
    const float prob_diffuse = info.roughness;
    const bool choose_diffuse = Random::rnd(payload.random_seed) < prob_diffuse;

    // 根据选择的策略采样方向
    if (choose_diffuse)
    {
        // Diffuse 采样：cos-weighted hemisphere
        const float3 local_dir = Sample::get_cos_hemisphere_sample(payload.random_seed);
        float3 tangent, bitangent;
        Sample::create_local_coord(info.forward_normal, tangent, bitangent);
        out_dir = normalize(local_dir.x * tangent + local_dir.y * bitangent + local_dir.z * info.forward_normal);
    }
    else
    {
        // NDF 采样：GGX
        const float reflectance = 0.8f;
        const float3 f0 = lerp(0.16 * reflectance * reflectance, info.base_color, info.metallic);
        const float real_roughness = info.roughness * info.roughness;
        float3 h = SpecularBrdf::ggx_sample(payload.random_seed, real_roughness, info.forward_normal);
        out_dir = reflect(-view_dir, h);
    }

    // 检查采样方向是否有效（在表面上方）
    const float ndotl = dot(info.forward_normal, out_dir);
    if (ndotl <= 0.f)
    {
        out_throughput = float3(0.f);
        out_brdf_pdf = 1.f;
        out_origin = info.position;
        return;
    }

    // 计算完整的 BRDF = diffuse + specular（使用 eval_brdf）
    const float3 brdf_cos = eval_brdf(info, view_dir, out_dir);

    // 计算混合 PDF（使用 eval_brdf_pdf 保证一致性）
    out_brdf_pdf = eval_brdf_pdf(info, view_dir, out_dir);

    // throughput = BRDF * cos / PDF（带数值保护）
    out_throughput = brdf_cos / max(out_brdf_pdf, 1e-7f);

    out_origin = info.position + 0.0001f * out_dir;
}

/// 处理材质并采样下一条光线
/// @param payload 光线载荷
/// @param info 命中信息
/// @param ray_dir 当前光线方向
/// @param out_origin 输出：下一条光线起点
/// @param out_dir 输出：下一条光线方向
/// @param out_throughput 输出：throughput
/// @param out_emission 输出：自发光
/// @param out_done 输出：是否终止
/// @param out_brdf_pdf 输出：BRDF 采样 PDF（用于 MIS）
void process_material(
    inout HitPayload payload,
    const HitInfo info,
    const float3 ray_dir,
    out float3 out_origin,
    out float3 out_dir,
    out float3 out_throughput,
    out float3 out_emission,
    out bool out_done,
    out float out_brdf_pdf
)
{
    // 初始化输出
    out_emission = float3(0.f);
    out_throughput = float3(1.f);
    out_done = false;
    out_origin = info.position;
    out_dir = ray_dir;
    out_brdf_pdf = 1.f; // Delta 路径的 PDF 设为 1（不参与 MIS）

    // 处理自发光材质
    if (handle_emissive(info, out_emission, out_done))
    {
        return;
    }

    // 根据材质类型分发处理
    switch (info.material_type)
    {
    case MaterialType::TRANSPARENT:
        handle_transparent(payload, info, ray_dir, out_origin, out_dir, out_throughput);
        // Delta 路径，out_brdf_pdf 保持为 1
        break;

    case MaterialType::SPECULAR:
        handle_specular(info, ray_dir, out_origin, out_dir, out_throughput);
        // Delta 路径，out_brdf_pdf 保持为 1
        break;

    case MaterialType::DIFFUSE:
    default:
        handle_diffuse(payload, info, ray_dir, out_origin, out_dir, out_throughput, out_brdf_pdf);
        break;
    }
}

// ============================================================================
// BRDF 评估函数（用于 NEE）
// ============================================================================

/// 计算给定方向的 BRDF PDF（用于 MIS）
/// @param info 命中信息
/// @param view_dir 观察方向（从命中点到相机）
/// @param light_dir 光照方向（从命中点到光源）
/// @return 混合 BRDF 的 PDF
float eval_brdf_pdf(const HitInfo info, const float3 view_dir, const float3 light_dir)
{
    // Delta 材质没有 PDF（不参与 NEE）
    if (info.is_delta_path())
    {
        return 0.f;
    }

    const float prob_diffuse = info.roughness;

    // 漫反射 PDF
    const float diffuse_pdf = DiffuseBrdf::pdf(info.forward_normal, light_dir);

    // 镜面反射 PDF
    const float3 h = normalize(view_dir + light_dir);
    const float reflectance = 0.8f;
    const float3 f0 = lerp(0.16 * reflectance * reflectance, info.base_color, info.metallic);
    const float real_roughness = info.roughness * info.roughness;
    SpecularBrdf specular_brdf = SpecularBrdf(f0, real_roughness, info.forward_normal, view_dir, h);
    const float specular_pdf = specular_brdf.ggx_pdf();

    // 混合 PDF
    return prob_diffuse * diffuse_pdf + (1.f - prob_diffuse) * specular_pdf;
}

/// 评估 BRDF 值（用于 NEE 直接光照计算）
/// @param info 命中信息
/// @param view_dir 观察方向（从命中点到相机）
/// @param light_dir 光照方向（从命中点到光源）
/// @return BRDF * cos(theta)
float3 eval_brdf(const HitInfo info, const float3 view_dir, const float3 light_dir)
{
    const float ndotl = max(dot(info.forward_normal, light_dir), 0.f);
    if (ndotl <= 0.f)
    {
        return float3(0.f);
    }

    // Diffuse BRDF 项
    const float3 diffuse_color = (1.f - info.metallic) * info.base_color;
    const float3 diffuse = diffuse_color / M_PI;

    // Specular BRDF 项
    const float3 h = normalize(view_dir + light_dir);
    const float reflectance = 0.8f;
    const float3 f0 = lerp(0.16 * reflectance * reflectance, info.base_color, info.metallic);
    const float real_roughness = info.roughness * info.roughness;
    SpecularBrdf specular_brdf = SpecularBrdf(f0, real_roughness, info.forward_normal, view_dir, h);
    const float3 specular = specular_brdf.brdf();

    return (diffuse + specular) * ndotl;
}

// ============================================================================
// 调试通道
// ============================================================================

/// 处理调试通道（首次命中）
/// @param info 命中信息
/// @param channel 调试通道号
/// @param out_color 输出颜色
/// @return true 表示是调试模式
/// @note 通道 0: 正常渲染
///       通道 1: 法线可视化
///       通道 2: 基础颜色
///       通道 4/5/6/7/8: 需要完整路径追踪，在此函数中不处理
///       通道 9: 首次命中位置的 IC 查询结果（需要完整路径追踪）
bool handle_debug_channel(const HitInfo info, uint channel, out float3 out_color)
{
    out_color = float3(0.f);

    if (channel == 0)
    {
        return false;
    }

    // 通道 1：法线可视化
    if (channel == 1)
    {
        out_color = info.forward_normal * 0.5f + 0.5f;
        return true;
    }

    // 通道 2：基础颜色
    if (channel == 2)
    {
        out_color = info.base_color;
        return true;
    }

    return false;
}

// ============================================================================
// SER（Shader Execution Reordering）
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

/// 发射 shadow ray，判断是否命中任何物体
/// @return true 表示命中任何物体，false 表示未命中
bool shadow_ray_any_hit(RaytracingAccelerationStructure tlas, const RayDesc ray)
{
    RayQuery<RAY_FLAG_ACCEPT_FIRST_HIT_AND_END_SEARCH | RAY_FLAG_SKIP_CLOSEST_HIT_SHADER> rq;
    rq.TraceRayInline(
        tlas,
        RAY_FLAG_NONE, // 前面已经限制了 flag
        0xFF,          // cull mask
        ray
    );
    while (rq.Proceed())
    {
        if (rq.CandidateType() == CANDIDATE_NON_OPAQUE_TRIANGLE)
        {
            rq.CommitNonOpaqueTriangleHit();
        }
        // if (rq.CommittedStatus() != COMMITTED_NOTHING)
        // {
        //     return false; // hit
        // }
    }
    return rq.CommittedStatus() == COMMITTED_TRIANGLE_HIT; // miss
}

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

    // 路径追踪主循环
    float3 radiance = float3(0.f);
    float3 throughput = float3(1.f);
    float prev_brdf_pdf = 1.f;
    bool has_diffuse = false;  // 跟踪路径上是否遇到了 diffuse
    bool prev_is_delta = true; // 上一次是否为 delta 路径（首次弹射视为 delta）

    // 调试通道 4/5/6/7/8：分离的 radiance 累积
    float3 nee_radiance = float3(0.f);         // 通道 4：NEE 命中 HDRI 的贡献
    float3 emissive_radiance = float3(0.f);    // 通道 5：自发光物体的贡献
    float3 hdri_radiance = float3(0.f);        // 通道 6：最终命中 HDRI 的贡献
    float3 nee_bounce1_radiance = float3(0.f); // 通道 7：第一次 bounce 的 NEE 贡献
    float3 nee_bounce2_radiance = float3(0.f); // 通道 8：第二次 bounce 的 NEE 贡献

    // 调试通道 9：首次命中位置的 IC 查询
    float3 first_hit_position = float3(0.f);
    float3 first_hit_normal = float3(0.f);
    bool first_hit_recorded = false;

    // ========== GBuffer 跟踪变量 ==========
    bool gbuffer_written = false; // GBuffer 是否已写入

    // ========== Irradiance Cache 局部状态 ==========
    uint ic_diffuse_bounce_count = 0;   // Diffuse 弹射计数
    bool ic_pending_update = false;     // 是否有待处理的 IC 更新操作
    bool ic_pending_insert = false;     // 是否有待处理的 IC 插入操作
    bool has_ic_query = false;          // 是否已进行 IC 查询
    uint ic_entry_index = 0xFFFFFFFF;   // 待更新的 IC entry 索引
    float3 ic_position = float3(0.f);   // IC 记录点的世界位置
    float3 ic_normal = float3(0.f);     // IC 记录点的法线
    float3 ic_throughput = float3(1.f); // 从 IC 记录点开始的相对 throughput
    float3 ic_radiance = float3(0.f);   // 累积的 outgoing radiance（已含 diffuse BRDF）

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

        // 调试通道
        float3 debug_color;
        if (handle_debug_channel(payload.info, push_const.channel, debug_color))
        {
            radiance = debug_color;
            break;
        }

        // ================================================================
        // IC 追踪模式：强制 diffuse 材质
        // 当 IC 启用且满足 diffuse 条件时，将材质强制为纯 diffuse
        // 以提高 IC 缓存的一致性
        // ================================================================
        if (payload.hit &&
            push_const.ic_enabled != 0 &&
            ic_diffuse_bounce_count >= rt::IC_MIN_DIFFUSE_BOUNCES &&
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
            // sky_color 已在 miss shader 中采样并存储于 payload.info.emissive
            const float3 sky_color = payload.info.emissive;

            // GBuffer: miss 情况写入默认值
            if (!gbuffer_written)
            {
                gbuffer::write_gbuffer_miss(thread_id);
                // miss 像素也必须写入 DLSS 输入，避免 SR 读取上一帧残留的 depth/mvec。
                gbuffer::write_dlss_sr_miss(thread_id);
                gbuffer_written = true;
            }

            if (prev_is_delta || depth == 0)
            {
                // Delta 路径或首次弹射：直接累加，不使用 MIS
                const float3 contrib = sky_color * throughput;
                radiance += contrib;
                hdri_radiance += contrib; // 调试通道 6
                // IC 追踪：累积环境光贡献
                if (ic_pending_update || ic_pending_insert)
                {
                    ic_radiance += sky_color * ic_throughput;
                }
            }
            else
            {
                // 非 delta 路径：使用 MIS 加权
                const float env_pdf = env_map_uniform_pdf();
                const float mis_weight = MIS::power_heuristic(prev_brdf_pdf, env_pdf);
                const float3 contrib = sky_color * throughput * mis_weight;
                radiance += contrib;
                hdri_radiance += contrib; // 调试通道 6
                // IC 追踪：累积环境光贡献（使用相同 MIS 权重）
                if (ic_pending_update || ic_pending_insert)
                {
                    ic_radiance += sky_color * ic_throughput * mis_weight;
                }
            }
            break;
        }

        // ================================================================
        // 记录首次命中位置（用于调试通道 9）
        // ================================================================
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
            // 线性深度使用光线 t 值（从 hit shader 传递）
            const float linear_depth = payload.info.hit_t;

            gbuffer::GBufferData gbuffer_data = gbuffer::make_gbuffer_data(
                payload.info.position,
                payload.info.forward_normal,
                linear_depth,
                payload.info.base_color,
                payload.info.metallic,
                payload.info.roughness
            );
            gbuffer::write_gbuffer_hit(thread_id, gbuffer_data);
            // DLSS depth 与 GBuffer linear_depth 分开写，保证 SR 输入和 debug GBuffer 各自语义稳定。
            gbuffer::write_dlss_sr_hit(thread_id, compute_device_depth(payload.info.position));
            gbuffer_written = true;
        }

        // ================================================================
        // 处理自发光物体（不使用 MIS）
        // ================================================================
        if (payload.info.material_type == MaterialType::EMISSIVE)
        {
            // 自发光物体：直接累加，不使用 MIS 权重
            const float3 contrib = payload.info.emissive * throughput;
            radiance += contrib;
            emissive_radiance += contrib; // 调试通道 5
            // IC 追踪：累积自发光贡献
            if (ic_pending_update || ic_pending_insert)
            {
                ic_radiance += payload.info.emissive * ic_throughput;
            }
            break;
        }

        // ================================================================
        // NEE: 下一事件估计（仅对非 delta 材质）
        // ================================================================
        if (!payload.info.is_delta_path())
        {
            // 在 HDRI 上均匀采样光源方向
            float3 light_dir;
            float env_pdf;
            sample_env_map_uniform(payload.random_seed, light_dir, env_pdf);

            // 检查光源方向是否在表面上方
            const float ndotl = dot(payload.info.forward_normal, light_dir);
            if (ndotl > 0.f)
            {
                // 构造阴影光线
                RayDesc shadow_ray_desc;
                shadow_ray_desc.Origin = payload.info.position + 0.001f * light_dir;
                shadow_ray_desc.Direction = light_dir;
                shadow_ray_desc.TMin = 0.001f;
                shadow_ray_desc.TMax = 10000.0f;

                // 检测可见性
                if (!shadow_ray_any_hit(rt::rt_tlas, shadow_ray_desc))
                {
                    // 可见：计算直接光照贡献
                    const float2 env_uv = dir_to_env_uv(light_dir);
                    const float3 Li = bindless_srv::sample_level(gpu_scene.sky, env_uv, gpu_scene.sky_sampler_type, 0.0).xyz * 8.f;

                    const float3 view_dir = -ray.Direction;
                    const float3 brdf_cos = eval_brdf(payload.info, view_dir, light_dir);
                    const float brdf_pdf = eval_brdf_pdf(payload.info, view_dir, light_dir);

                    // MIS 权重（光源采样策略）
                    const float mis_weight = MIS::power_heuristic(env_pdf, brdf_pdf);

                    // 累加 NEE 贡献：throughput * Li * BRDF * cos / env_pdf * mis_weight
                    const float3 nee_contrib = throughput * Li * brdf_cos / max(env_pdf, 1e-7f) * mis_weight;
                    radiance += nee_contrib;
                    nee_radiance += nee_contrib; // 调试通道 4
                    // IC 追踪：累积 NEE 贡献（使用相对 throughput）
                    if (ic_pending_update || ic_pending_insert)
                    {
                        ic_radiance += ic_throughput * Li * brdf_cos / max(env_pdf, 1e-7f) * mis_weight;
                    }
                    if (depth == 0)
                    {
                        nee_bounce1_radiance = nee_contrib; // 调试通道 7
                    }
                    else if (depth == 1)
                    {
                        nee_bounce2_radiance = nee_contrib; // 调试通道 8
                    }
                }
            }
        }

        // ================================================================
        // Irradiance Cache 查询（NEE 之后，BRDF 采样之前）
        // 条件：bounces、当前为 diffuse 表面、无待处理 IC 操作、IC 已启用
        // ================================================================
        if (
            push_const.ic_enabled != 0 &&
            ic_diffuse_bounce_count >= rt::IC_MIN_DIFFUSE_BOUNCES &&
            ic::is_diffuse_surface(payload.info.roughness, payload.info.metallic) &&
            !(ic_pending_update || ic_pending_insert || has_ic_query)
        )
        {
            float3 ic_query_radiance;
            float ic_weight;
            uint ic_max_weight_entry_index;
            float ic_max_weight;

            bool ic_found = ic::fn::lookup(
                push_const.ic_table,
                push_const.ic_entry_pool,
                payload.info.position,
                payload.info.forward_normal,
                ic_query_radiance,
                ic_weight,
                ic_max_weight_entry_index,
                ic_max_weight
            );
            has_ic_query = true;

            if (ic_found && ic_max_weight > ic::IC_LOOKUP_THRESHOLD)
            {
                // 概率性使用 IC 结果
                if (Random::rnd(payload.random_seed) < 0.7f)
                {
                    // IC 查询成功且权重足够高：直接使用 IC 结果并终止光路
                    // IC 里面存的是 outgoing radiance，因此可以直接累加
                    radiance += throughput * ic_query_radiance;
                    break;
                }
            }

            if (ic_found && ic_max_weight > ic::IC_UPDATE_THRESHOLD)
            // if (ic_found && ic_max_weight > 0.f)
            {
                // IC 查询成功但权重较低：标记待更新，继续追踪以获取更好的估计
                ic_pending_update = true;
                ic_entry_index = ic_max_weight_entry_index;
                ic_position = payload.info.position;
                ic_normal = payload.info.forward_normal;
                // 初始化相对 throughput 和 radiance，不受 RR 缩放影响
                ic_throughput = float3(1.f);
                ic_radiance = float3(0.f);
            }
            else if (!ic_found && push_const.spp_idx == 0)
            {
                // IC 查询失败：仅在 spp_idx == 0 时标记待插入，限制插入频率
                ic_pending_insert = true;
                ic_position = payload.info.position;
                ic_normal = payload.info.forward_normal;
                // 初始化相对 throughput 和 radiance，不受 RR 缩放影响
                ic_throughput = float3(1.f);
                ic_radiance = float3(0.f);
            }
        }

        // ================================================================
        // 处理材质，计算下一条光线（BRDF 采样）
        // ================================================================
        float3 next_origin, next_dir, mat_throughput, emission;
        bool done;
        float brdf_pdf;
        process_material(
            payload,
            payload.info,
            ray.Direction,
            next_origin,
            next_dir,
            mat_throughput,
            emission,
            done,
            brdf_pdf
        );

        // 累积自发光（如 SKY 材质类型）
        radiance += emission * throughput;

        if (done)
        {
            break;
        }

        // 更新 throughput
        throughput *= mat_throughput;
        // IC 追踪：同步更新 ic_throughput（不受后续 RR 缩放影响）
        if (ic_pending_update || ic_pending_insert)
        {
            ic_throughput *= mat_throughput;
        }

        // 记录当前路径信息供下次 MIS 使用
        prev_brdf_pdf = brdf_pdf;
        prev_is_delta = payload.info.is_delta_path();

        // 检测是否遇到了 diffuse（在 BRDF 采样之后）
        if (ic::is_diffuse_surface(payload.info.roughness, payload.info.metallic))
        {
            has_diffuse = true;
            // 递增 diffuse 弹射计数（用于 IC 查询条件）
            ic_diffuse_bounce_count++;
        }

        if (depth >= 3)
        {
            float p = max(throughput.x, max(throughput.y, throughput.z));
            p = clamp(p, 0.05f, 0.95f);
            if (Random::rnd(payload.random_seed) > p)
            {
                break;
            }
            throughput /= p;
            ic_throughput /= p;
        }

        // 设置下一条光线
        ray.Origin = next_origin;
        ray.Direction = next_dir;
    }

    // ================================================================
    // Irradiance Cache 更新/插入（路径追踪结束后）
    // 使用最终 radiance 和记录点的 throughput 估算 irradiance
    // 没有命中的路径也需要更新，因为这是 RR + MC 估计下的结果
    // ================================================================
    if (push_const.ic_enabled != 0 && (ic_pending_update || ic_pending_insert))
    {
        float max_ic_throughput = max(ic_throughput.r, max(ic_throughput.g, ic_throughput.b));
        float max_ic_radiance = max(ic_radiance.x, max(ic_radiance.y, ic_radiance.z));

        // 仅当 throughput 足够大时才进行 IC 操作
        // if (max_ic_throughput >= rt::IC_THROUGHPUT_MIN)
        {
            if (ic_pending_update)
            {
                // 更新现有 IC entry
                ic::fn::update(
                    push_const.ic_entry_pool,
                    ic_entry_index,
                    ic_position,
                    ic_normal,
                    ic_radiance
                );
            }
            else if (ic_pending_insert && push_const.spp_idx == 0)
            {
                // 插入新 IC entry（仅 spp_idx == 0 时执行，限制插入频率）
                ic::fn::insert(
                    push_const.ic_table,
                    push_const.ic_entry_pool,
                    ic_position,
                    ic_normal,
                    ic_radiance
                );
            }
        }
    }

    // 根据调试通道选择输出
    float3 output_radiance = radiance;
    if (push_const.channel == 4)
    {
        output_radiance = nee_radiance; // NEE 命中 HDRI 的贡献
    }
    else if (push_const.channel == 5)
    {
        output_radiance = emissive_radiance; // 自发光物体的贡献
    }
    else if (push_const.channel == 6)
    {
        output_radiance = hdri_radiance; // 最终命中 HDRI 的贡献
    }
    else if (push_const.channel == 7)
    {
        output_radiance = nee_bounce1_radiance; // 第一次 bounce 的 NEE 贡献
    }
    else if (push_const.channel == 8)
    {
        output_radiance = nee_bounce2_radiance; // 第二次 bounce 的 NEE 贡献
    }
    else if (push_const.channel == 9)
    {
        // 通道 9：使用首次命中位置查询 IC
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

            if (ic_found)
            {
                output_radiance = ic_query_radiance;
            }
            else
            {
                output_radiance = float3(0.f); // IC 查询失败，输出黑色
            }
        }
        else
        {
            output_radiance = float3(0.f); // 无首次命中，输出黑色
        }
    }

    // 输出单帧结果（累积逻辑移至单独的 accum pass）
    rt::rt_single_frame_output.Store(thread_id, float4(output_radiance, 1.f));
}
