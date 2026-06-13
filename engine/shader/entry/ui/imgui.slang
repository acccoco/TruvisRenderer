#include "api/pass/ui_imgui.slangi"
#include "lib/core/bindless.slangi"

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
        input.coarse_vertex.color *
        BindlessSrv::sample_level(
            push_constant.texture,
            input.coarse_vertex.uv,
            push_constant.texture_sampler_type,
            0.0
        );
    return output;
}
