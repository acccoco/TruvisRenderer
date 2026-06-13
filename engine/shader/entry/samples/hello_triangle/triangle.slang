#include "lib/core/vertex_attribute.slangi"

// 定义矩形的 6 个顶点（两个三角形）
static const float2 positions[6] = {
    float2(-0.5, -0.5), // 左下
    float2(0.5, -0.5),  // 右下
    float2(-0.5, 0.5),  // 左上
    float2(-0.5, 0.5),  // 左上
    float2(0.5, -0.5),  // 右下
    float2(0.5, 0.5)    // 右上
};

static const float2 uvs[6] = {
    float2(0.0, 0.0),
    float2(1.0, 0.0),
    float2(0.0, 1.0),
    float2(0.0, 1.0),
    float2(1.0, 0.0),
    float2(1.0, 1.0)
};

static const float3 normals[6] = {
    float3(0.0, 0.0, 1.0),
    float3(0.0, 0.0, 1.0),
    float3(0.0, 0.0, 1.0),
    float3(0.0, 0.0, 1.0),
    float3(0.0, 0.0, 1.0),
    float3(0.0, 0.0, 1.0)
};

static const float3 tangents[6] = {
    float3(1.0, 0.0, 0.0),
    float3(1.0, 0.0, 0.0),
    float3(1.0, 0.0, 0.0),
    float3(1.0, 0.0, 0.0),
    float3(1.0, 0.0, 0.0),
    float3(1.0, 0.0, 0.0)
};

[shader("vertex")]
soa::VsOutput vsmain(uint VertexIndex: SV_VertexID)
{
    soa::VsOutput output = (soa::VsOutput)0;
    output.coarse_vertex.UV = uvs[VertexIndex];
    output.coarse_vertex.Normal = normals[VertexIndex];
    output.coarse_vertex.Tangent = tangents[VertexIndex];
    output.Pos = float4(positions[VertexIndex], 0.0, 1.0);
    return output;
}

[shader("pixel")]
soa::PS_OUTPUT psmain(soa::CoarseVertex input)
{
    soa::PS_OUTPUT output = (soa::PS_OUTPUT)0;
    output.Color = float4(
        input.UV.x,
        input.UV.y,
        1.f - (input.UV.x + input.UV.y) * 0.5f,
        1.0
    );
    return output;
}
