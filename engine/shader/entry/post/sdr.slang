#include "api/pass/sdr.slangi"
#include "lib/core/random.slangi"
#include "lib/core/bindless.slangi"

/// HDR -> SDR compute 入口。
///
/// Final channel 会做曝光、ACES fitted 近似和轻微 dithering；debug channel 直接透传 RGB，
/// 避免 normal/albedo 等诊断值被 tone mapping 改变含义。
[push_constant]
sdr::PushConstant g_params;

/// ACES fitted approximation。
///
/// 它不是完整 ACES/OCIO transform，只作为 realtime SDR preview 的高光压缩曲线。
float3 aces_fitted(float3 color)
{
    // 实时渲染常用的 ACES fitted approximation；这里只作为 SDR sRGB 输出路径的显示映射。
    const float a = 2.51f;
    const float b = 0.03f;
    const float c = 2.43f;
    const float d = 0.59f;
    const float e = 0.14f;
    return (color * (a * color + b)) / (color * (c * color + d) + e);
}

float3 tone_map_final(float3 hdr_color, float dither)
{
    // exposure_ev 是以 2^EV 缩放的手动曝光；负值压暗，正值提亮。
    const float3 exposed = max(hdr_color, float3(0.f)) * exp2(g_params.exposure_ev);
    const float white_point = max(g_params.aces_white_point, 1e-4f);
    const float3 white_scale = max(aces_fitted(float3(white_point)), float3(1e-4f));
    const float3 aces_color = saturate(aces_fitted(exposed) / white_scale);
    const float3 reinhard_color = exposed / (float3(1.f) + exposed);
    const float aces_strength = saturate(g_params.aces_strength);

    return saturate(lerp(reinhard_color, aces_color, aces_strength) + dither);
}

[shader("compute")]
[numthreads(sdr::SHADER_X, sdr::SHADER_Y, 1)]
void main(uint3 dispatchThreadID: SV_DispatchThreadID)
{
    // SR 开启时 src/dst extent 可能来自不同阶段；这里按传入的目标 image_size 做严格 OOB
    // 保护，避免 resize 后多余 dispatch thread 写到目标图像边界外。
    if (dispatchThreadID.x >= g_params.image_size.x ||
        dispatchThreadID.y >= g_params.image_size.y)
    {
        return; // Out of bounds
    }

    const float4 hdr_color = BindlessUav::load(g_params.src_image, dispatchThreadID.xy);

    uint random_seed = Random::tea(dispatchThreadID.y, dispatchThreadID.x);
    // 1/255 量级 dithering 用于减少 SDR 8-bit 显示上的 banding，不改变 HDR 中间结果。
    const float dither = (Random::rnd(random_seed) - 0.5f) / 255.f;
    float3 sdr_color;
    if (g_params.channel == 0)
    {
        sdr_color = tone_map_final(hdr_color.rgb, dither);
    }
    else
    {
        sdr_color = hdr_color.rgb;
    }

    BindlessUav::store(
        g_params.dst_image,
        dispatchThreadID.xy,
        float4(sdr_color, hdr_color.a)
    );
}
