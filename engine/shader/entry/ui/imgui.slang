#include "api/pass/ui_imgui.slangi"
#include "lib/core/bindless.slangi"

/// ImGui Vulkan backend shader。
///
/// 顶点输入来自每帧上传的 ImGui draw data，坐标已经是 display space；push constant 中的
/// ortho 矩阵把它转换为 clip space。pixel shader 将 vertex color 与 font/debug texture 相乘。
[[vk::push_constant]]
ui_imgui::PushConstant push_constant;

struct CoarseVertex
{

    [[vk::location(0)]]
    float4 color : COLOR;

    [[vk::location(1)]]
    float2 uv : UV;
};

struct VsInput
{
    [[vk::location(0)]]
    float2 pos : POS;

    [[vk::location(1)]]
    float2 uv : UV;

    [[vk::location(2)]]
    float4 color : COLOR;
};

struct VsOutput
{
    float4 pos : SV_Position;

    CoarseVertex coarse_vertex : COARSE_VERTEX;
};

[shader("vertex")]
VsOutput vsmain(VsInput input)
{
    VsOutput output = (VsOutput)0;
    output.coarse_vertex.color = input.color;
    output.coarse_vertex.uv = input.uv;
    // ImGui backend 负责构造正交矩阵，shader 不再知道窗口尺寸和 framebuffer scale。
    output.pos = mul(push_constant.ortho, float4(input.pos, 0.0, 1.0));
    return output;
}

struct PsInput
{
    CoarseVertex coarse_vertex : COARSE_VERTEX;
};

struct PsOutput
{
    [[vk::location(0)]]
    float4 color : SV_TARGET0;
};

[shader("pixel")]
PsOutput psmain(PsInput input)
{
    PsOutput output = (PsOutput)0;

    output.color =
        // ImGui 颜色已经包含 style/alpha，texture alpha 再决定字体或 image 的实际覆盖。
        input.coarse_vertex.color *
        BindlessSrv::sample_level(
            push_constant.texture,
            input.coarse_vertex.uv,
            push_constant.texture_sampler_type,
            0.0
        );
    return output;
}
