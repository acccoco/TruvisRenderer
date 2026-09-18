#include "lib/renderer/raster/material_vertex.slangi"
#include "lib/renderer/raster/phong.slangi"

/// position/normal 来自 vertex binding；四槽 UV 和切线由 PhongVertex 按 Geometry 元数据读取。
struct VsInput
{
    [[vk::location(0)]]
    float3 pos : LOCAL_POS;

    [[vk::location(1)]]
    float3 normal : NORMAL;
};

struct VsOutput
{
    float4 pos : SV_POSITION;

    CoarseVertex coarse_vertex : CoarseVertex;
};

[[vk::push_constant]]
renderer::render_passes::raster::PushConstants push_const;

[shader("vertex")]
VsOutput main(VsInput input, uint vertex_id : SV_VertexID)
{
    engine::scene::Instance* instance = SceneAccess::instance(push_const.instance_idx);

    VsOutput output = (VsOutput)0;

    const float4x4 mvp = mul(engine::binding::per_frame_data.projection, mul(engine::binding::per_frame_data.view, instance.model));
    output.pos = mul(mvp, float4(input.pos, 1.0));
    output.coarse_vertex = PhongVertex::evaluate(push_const, vertex_id, input.pos, input.normal);

    return output;
}
