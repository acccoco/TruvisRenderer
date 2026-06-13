/// @file denoise_accum.slang
/// @brief 降噪累积 Pass - 对单帧 RT 结果进行增强联合双边滤波降噪，然后累积到 accum_image 中
///
/// 增强联合双边滤波特性：
/// - 空间权重：基于像素距离的高斯权重
/// - 深度权重：深度差异越大权重越小
/// - 法线权重：法线夹角越大权重越小
/// - 世界空间位置权重：使用 world_position.xyz 计算欧氏距离，比纯深度更能保护边缘
/// - Albedo 引导权重：材质颜色相似性
/// - 粗糙度自适应：粗糙表面使用更大滤波核和更宽松的 sigma，光滑表面保留更多细节
///
/// 当 denoise_enabled == 0 时，跳过降噪直接累积
/// 当 accum_frames == 0 时，直接写入结果（不累积）
/// 否则执行 lerp(old_accum, denoised_frame, 1/(accum_frames+1)) 混合

#include "api/pass/denoise_accum.slangi"
#include "lib/core/bindless.slangi"

[push_constant]
denoise_accum::PushConstant g_params;

/// 计算空间高斯权重
float spatial_weight(float2 offset)
{
    // 固定 sigma_spatial = kernel_radius / 2
    float sigma_spatial = max(float(g_params.kernel_radius) * 0.5, 0.5);
    float dist_sq = dot(offset, offset);
    return exp(-dist_sq / (2.0 * sigma_spatial * sigma_spatial));
}

/// 计算颜色相似度权重
float color_weight(float3 color_center, float3 color_neighbor)
{
    float3 diff = color_center - color_neighbor;
    float dist_sq = dot(diff, diff);
    float sigma_sq = g_params.sigma_color * g_params.sigma_color;
    return exp(-dist_sq / (2.0 * sigma_sq + 1e-6));
}

/// 计算深度相似度权重
float depth_weight(float depth_center, float depth_neighbor)
{
    float diff = abs(depth_center - depth_neighbor);
    // 使用相对深度差异，避免远处物体权重过低
    float relative_diff = diff / (abs(depth_center) + 1e-6);
    float sigma_sq = g_params.sigma_depth * g_params.sigma_depth;
    return exp(-relative_diff * relative_diff / (2.0 * sigma_sq + 1e-6));
}

/// 计算法线相似度权重（支持粗糙度自适应 sigma）
float normal_weight(float3 normal_center, float3 normal_neighbor, float adaptive_sigma)
{
    // 使用 1 - dot 作为距离度量（dot = 1 时完全相同，dot = 0 时垂直）
    float ndot = max(dot(normal_center, normal_neighbor), 0.0);
    float diff = 1.0 - ndot;
    float sigma_sq = adaptive_sigma * adaptive_sigma;
    return exp(-diff * diff / (2.0 * sigma_sq + 1e-6));
}

/// 计算世界空间位置相似度权重
/// 使用归一化到 scene_scale 的欧氏距离
float position_weight(float3 pos_center, float3 pos_neighbor)
{
    float3 diff = pos_center - pos_neighbor;
    // 归一化到场景尺度
    float dist = length(diff) / (g_params.scene_scale + 1e-6);
    float sigma_sq = g_params.sigma_position * g_params.sigma_position;
    return exp(-dist * dist / (2.0 * sigma_sq + 1e-6));
}

/// 计算 Albedo 相似度权重
float albedo_weight(float3 albedo_center, float3 albedo_neighbor)
{
    float3 diff = albedo_center - albedo_neighbor;
    float dist_sq = dot(diff, diff);
    float sigma_sq = g_params.sigma_albedo * g_params.sigma_albedo;
    return exp(-dist_sq / (2.0 * sigma_sq + 1e-6));
}

/// 基于粗糙度计算自适应滤波半径
int adaptive_kernel_radius(float roughness)
{
    if (g_params.roughness_adaptive_enabled == 0)
        return g_params.kernel_radius;
    
    // 粗糙表面需要更大的滤波核
    // 粗糙度 接近 1 时，radius 增大
    // 粗糙度 接近 0 时，radius 减小（保留镜面细节）
    float scale = lerp(0.5, g_params.roughness_radius_scale, roughness);
    return max(1, int(float(g_params.kernel_radius) * scale));
}

/// 基于粗糙度计算自适应法线 sigma
float adaptive_sigma_normal(float roughness)
{
    if (g_params.roughness_adaptive_enabled == 0)
        return g_params.sigma_normal;
    
    // 光滑表面需要更严格的法线权重
    // 粗糙表面可以接受更大的法线偏差
    return g_params.sigma_normal * lerp(0.3, g_params.roughness_sigma_scale, roughness);
}

/// 执行增强联合双边滤波降噪
float4 bilateral_filter(uint2 pixel, float4 center_color, float3 center_normal, 
                        float center_depth, float3 center_position, float3 center_albedo, 
                        float center_roughness)
{
    float3 color_sum = float3(0.0, 0.0, 0.0);
    float weight_sum = 0.0;
    
    // 基于粗糙度计算自适应参数
    int radius = adaptive_kernel_radius(center_roughness);
    float sigma_normal_adaptive = adaptive_sigma_normal(center_roughness);
    
    for (int dy = -radius; dy <= radius; dy++)
    {
        for (int dx = -radius; dx <= radius; dx++)
        {
            int2 neighbor_pixel = int2(pixel) + int2(dx, dy);
            
            // 边界检查
            if (neighbor_pixel.x < 0 || neighbor_pixel.x >= int(g_params.image_size.x) ||
                neighbor_pixel.y < 0 || neighbor_pixel.y >= int(g_params.image_size.y))
            {
                continue;
            }
            
            uint2 np = uint2(neighbor_pixel);
            
            // 读取邻居像素数据
            float4 neighbor_color = BindlessUav::load(g_params.single_frame_input, np);
            float4 neighbor_gbuffer_a = BindlessUav::load(g_params.gbuffer_a, np);
            float4 neighbor_gbuffer_b = BindlessUav::load(g_params.gbuffer_b, np);
            float4 neighbor_gbuffer_c = BindlessUav::load(g_params.gbuffer_c, np);
            
            float3 neighbor_normal = neighbor_gbuffer_a.xyz;
            float neighbor_depth = neighbor_gbuffer_b.w;
            float3 neighbor_position = neighbor_gbuffer_b.xyz;
            float3 neighbor_albedo = neighbor_gbuffer_c.xyz;
            
            // 计算各项权重
            float w_spatial = spatial_weight(float2(dx, dy));
            float w_color = color_weight(center_color.rgb, neighbor_color.rgb);
            float w_depth = depth_weight(center_depth, neighbor_depth);
            float w_normal = normal_weight(center_normal, neighbor_normal, sigma_normal_adaptive);
            float w_position = position_weight(center_position, neighbor_position);
            float w_albedo = albedo_weight(center_albedo, neighbor_albedo);
            
            // 组合权重策略：
            // - 空间权重：基础高斯
            // - 几何权重：position * normal * depth（保护边缘，depth 作为备份）
            // - 材质权重：max(color, albedo)（避免过度平滑）
            float w_geometry = w_position * w_normal * w_depth;
            float w_material = max(w_color, w_albedo);
            float weight = w_spatial * w_geometry * w_material;
            
            color_sum += neighbor_color.rgb * weight;
            weight_sum += weight;
        }
    }
    
    // 归一化
    if (weight_sum > 1e-6)
    {
        return float4(color_sum / weight_sum, center_color.a);
    }
    else
    {
        return center_color;
    }
}

[shader("compute")]
[numthreads(denoise_accum::SHADER_X, denoise_accum::SHADER_Y, 1)]
void main(uint3 dispatchThreadID: SV_DispatchThreadID)
{
    if (dispatchThreadID.x >= g_params.image_size.x ||
        dispatchThreadID.y >= g_params.image_size.y)
    {
        return; // Out of bounds
    }

    uint2 pixel = dispatchThreadID.xy;
    
    // 读取当前帧的单帧 RT 输出
    float4 current_frame = BindlessUav::load(g_params.single_frame_input, pixel);
    
    // 降噪处理
    float4 denoised_frame;
    if (g_params.denoise_enabled != 0)
    {
        // 读取 GBuffer 数据
        float4 gbuffer_a = BindlessUav::load(g_params.gbuffer_a, pixel);
        float4 gbuffer_b = BindlessUav::load(g_params.gbuffer_b, pixel);
        float4 gbuffer_c = BindlessUav::load(g_params.gbuffer_c, pixel);
        
        float3 center_normal = gbuffer_a.xyz;
        float center_roughness = gbuffer_a.w;
        float3 center_position = gbuffer_b.xyz;
        float center_depth = gbuffer_b.w;
        float3 center_albedo = gbuffer_c.xyz;
        
        // 如果深度为默认值（miss），跳过降噪
        if (center_depth >= 9999.0)
        {
            denoised_frame = current_frame;
        }
        else
        {
            denoised_frame = bilateral_filter(pixel, current_frame, center_normal, 
                                              center_depth, center_position, center_albedo,
                                              center_roughness);
        }
    }
    else
    {
        // 降噪禁用，直接使用原始帧
        denoised_frame = current_frame;
    }
    
    // 累积处理
    float4 result;
    if (g_params.channel == 3 || g_params.accum_frames == 0)
    {
        // legacy pass-local 调试通道 3 或第一帧：直接写入降噪/未降噪结果
        result = denoised_frame;
    }
    else
    {
        // 读取累积图像中的旧值
        float4 old_accum = BindlessUav::load(g_params.accum_output, pixel);
        
        // 计算混合因子: 1 / (accum_frames + 1)
        float blend = 1.0 / float(g_params.accum_frames + 1);
        
        // 线性混合
        result = lerp(old_accum, denoised_frame, blend);
    }
    
    // 写入累积结果
    BindlessUav::store(g_params.accum_output, pixel, result);
}
