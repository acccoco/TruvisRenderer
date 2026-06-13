#include "api/pass/raster.slangi"
#include "./phong.slangi"

struct VsInput
{
    [[vk::location(0)]]
    float3 pos : LOCAL_POS;

    [[vk::location(1)]]
    float3 normal : NORMAL;

    [[vk::location(2)]]
    float2 uv : UV;
};

struct VsOutput
{
    float4 pos : SV_POSITION;

    CoarseVertex coarse_vertex : CoarseVertex;
};

[[vk::push_constant]]
raster::PushConstants push_const;

[shader("vertex")]
VsOutput main(VsInput input)
{
    scene::Instance* instance = push_const.scene.all_instances + push_const.instance_idx;
    frame::PerFrameData* frame_data = push_const.frame_data;

    VsOutput output = (VsOutput)0;

    const float4x4 mvp = mul(frame_data.projection, mul(frame_data.view, instance.model));
    output.pos = mul(mvp, float4(input.pos, 1.0));
    output.coarse_vertex.world_pos = mul(instance.model, float4(input.pos, 1.0)).xyz;
    output.coarse_vertex.uv = input.uv;
    output.coarse_vertex.frag_normal = mul(instance.inv_model, float4(input.normal, 0.0)).xyz;

    return output;
}
