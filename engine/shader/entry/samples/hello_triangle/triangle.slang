#include "lib/core/vertex_attribute.slangi"

/// Hello Triangle sample 的自包含 shader。
///
/// 该 sample 不读取 scene、descriptor 或 push constant，只用 `SV_VertexID` 生成一个矩形，
/// 用于验证 swapchain、graphics pipeline、RenderGraph present pass 和基础 vertex/fragment 流程。

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
    // 顶点属性完全由常量数组提供，避免 sample 依赖任何 GPU buffer 上传路径。
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
    // 用 UV 构造可视化颜色，便于验证顶点顺序、插值和 fragment 输出。
    output.Color = float4(
        input.UV.x,
        input.UV.y,
        1.f - (input.UV.x + input.UV.y) * 0.5f,
        1.0
    );
    return output;
}
