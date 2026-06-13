#include "api/pass/resolve.slangi"
#include "lib/core/bindless.slangi"

/// graphics resolve 入口。
///
/// vertex shader 在目标 color attachment 上生成一个指定像素区域的 quad；fragment shader 从
/// bindless SRV 采样源图像。它用于把 main view、DLSS output 或 debug image 合成到 swapchain。
[[vk::push_constant]]
resolve::PushConstant g_params;

struct VsOutput
{
    float4 position : SV_Position;
    float2 uv : TEXCOORD0;
};

/// 顶点着色器
/// 输入：固定的边长为1的正方形顶点（位置 + UV）
/// 根据 push constant 中的 offset 和 size 计算最终的 NDC 坐标
[shader("vertex")]
VsOutput vsmain(uint vertexID: SV_VertexID)
{
    // 固定的单位正方形顶点：两个三角形覆盖 [0,1] x [0,1]
    // 三角形顺序：(0,1,2), (2,3,0) 对应 CCW
    // 0: (0, 0), 1: (1, 0), 2: (1, 1), 3: (0, 1)
    const float2 positions[4] = {
        float2(0.0, 0.0), // A (左下)
        float2(1.0, 0.0), // B (右下)
        float2(1.0, 1.0), // C (右上)
        float2(0.0, 1.0), // D (左上)
    };

    // V 坐标翻转：Vulkan 帧缓冲原点在左上角，而纹理采样 (0,0) 在左上角
    // 但由于 viewport Y 翻转，需要将 V 坐标反转
    const float2 uvs[4] = {
        float2(0.0, 1.0), // A (左下 . V=1)
        float2(1.0, 1.0), // B (右下 . V=1)
        float2(1.0, 0.0), // C (右上 . V=0)
        float2(0.0, 0.0), // D (左上 . V=0)
    };

    // 索引顺序：0, 1, 2, 2, 3, 0 (两个三角形)
    const uint indices[6] = { 0, 1, 2, 2, 3, 0 };
    uint idx = indices[vertexID];

    float2 localPos = positions[idx];
    float2 uv = uvs[idx];

    // 将 [0,1] 范围内的位置转换到目标区域
    // 先缩放到指定 size，再偏移到指定 offset
    float2 pixelPos = localPos * g_params.size + g_params.offset;

    // 将像素坐标转换为 NDC 坐标 [-1, 1]
    // NDC.x = (pixelPos.x / target_size.x) * 2.0 - 1.0
    // NDC.y = (pixelPos.y / target_size.y) * 2.0 - 1.0
    // 这里依赖上层 viewport/scissor 已匹配目标 attachment，不在 shader 内做裁剪。
    float2 ndc = (pixelPos / g_params.target_size) * 2.0 - 1.0;

    VsOutput output;
    output.position = float4(ndc, 0.0, 1.0);
    output.uv = uv;

    return output;
}

/// 片段着色器
/// 采样源图像并输出到 color attachment
[shader("fragment")]
float4 psmain(VsOutput input)
    : SV_Target
{
    // 使用 NonUniformResourceIndex 处理 divergent indexing
    return BindlessSrv::sample(
        g_params.src_texture,
        input.uv,
        g_params.sampler_type
    );
}
