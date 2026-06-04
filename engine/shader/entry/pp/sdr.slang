#include "share/pass/sdr.slangi"
#include "lib/sample/random.slangi"
#include "lib/bindless_op.slangi"

[push_constant]
sdr::PushConstant g_params;

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

    const float4 hdr_color = bindless_uav::load(g_params.src_image, dispatchThreadID.xy);

    // TODO 引入 accum frame 的参数，增加随机性
    // 引入噪声消除色阶
    uint random_seed = Random::tea(dispatchThreadID.y, dispatchThreadID.x);
    const float delta = Random::rnd(random_seed);
    float3 sdr_color;
    if (g_params.channel == 0)
    {
        sdr_color = hdr_color.rgb / (1.f + hdr_color.rgb) + delta / 255.f;
    }
    else
    {
        sdr_color = hdr_color.rgb;
    }

    bindless_uav::store(
        g_params.dst_image,
        dispatchThreadID.xy,
        float4(sdr_color, hdr_color.a)
    );
}
