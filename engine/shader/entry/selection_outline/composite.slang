#include "api/pass/selection_outline.slangi"

[[vk::push_constant]]
selection_outline::CompositePushConstant g_params;

struct VsOutput
{
    float4 position : SV_Position;
    float2 uv : TEXCOORD0;
};

[shader("vertex")]
VsOutput vsmain(uint vertexID : SV_VertexID)
{
    const float2 positions[4] = {
        float2(0.0, 0.0),
        float2(1.0, 0.0),
        float2(1.0, 1.0),
        float2(0.0, 1.0),
    };
    const float2 uvs[4] = {
        float2(0.0, 1.0),
        float2(1.0, 1.0),
        float2(1.0, 0.0),
        float2(0.0, 0.0),
    };
    const uint indices[6] = { 0, 1, 2, 2, 3, 0 };
    const uint idx = indices[vertexID];

    const float2 ndc = positions[idx] * 2.0 - 1.0;

    VsOutput output = (VsOutput)0;
    output.position = float4(ndc, 0.0, 1.0);
    output.uv = uvs[idx];
    return output;
}

float mask_at(float2 uv)
{
    return selection_outline::mask_texture.Sample(global_samplers[bindless::ESamplerType::PointClamp], uv);
}

[shader("fragment")]
float4 psmain(VsOutput input) : SV_Target
{
    const float2 pixel = 1.0 / max(g_params.target_size, float2(1.0, 1.0));
    const float radius = max(g_params.radius_px, 1.0);
    const float center = mask_at(input.uv);

    float neighbor = center;
    neighbor = max(neighbor, mask_at(input.uv + float2( radius, 0.0) * pixel));
    neighbor = max(neighbor, mask_at(input.uv + float2(-radius, 0.0) * pixel));
    neighbor = max(neighbor, mask_at(input.uv + float2(0.0,  radius) * pixel));
    neighbor = max(neighbor, mask_at(input.uv + float2(0.0, -radius) * pixel));
    neighbor = max(neighbor, mask_at(input.uv + float2( radius,  radius) * pixel));
    neighbor = max(neighbor, mask_at(input.uv + float2(-radius,  radius) * pixel));
    neighbor = max(neighbor, mask_at(input.uv + float2( radius, -radius) * pixel));
    neighbor = max(neighbor, mask_at(input.uv + float2(-radius, -radius) * pixel));

    // 只在 mask 外侧输出轮廓，选中物体内部保持 resolve 结果不变。
    const float edge = saturate(neighbor - center);
    return float4(g_params.color.rgb, g_params.color.a * edge);
}
