#include "./phong.slangi"
#include "api/pass/raster.slangi"
#include "lib/core/bindless.slangi"
#include "lib/lighting/phong.slangi"
#include "lib/scene/scene_access.slangi"

/// Phong raster pixel shader。
///
/// 这是示例/调试光栅路径，不参与主 RT 的 PBR 管线。它通过 `SceneAccess` 读取当前 submesh
/// material，并遍历 scene 中的点光源做简化 Phong 光照。
struct PsInput
{
    CoarseVertex coarse_vertex : CoarseVertex;
};

struct PsOutput
{
    [[vk::location(0)]]
    float4 color : SV_TARGET0;
};

[[vk::push_constant]]
raster::PushConstants push_const;

[shader("pixel")]
PsOutput main(PsInput input)
{
    const float3 normal = normalize(input.coarse_vertex.frag_normal);

    frame::PerFrameData* frame_data = push_const.frame_data;
    scene::GpuScene* scene = push_const.scene;
    material::PbrMaterial* mat = SceneAccess::get_material(scene, push_const.instance_idx, push_const.submesh_idx);

    const uint light_cnt = scene.point_light_count;

    const float2 delta_uv = frac(frame_data.time_ms / 1000.0f);
    // 让示例纹理随时间滚动，便于确认 UV、sampler 和 bindless SRV 都在工作。
    const float2 uv = input.coarse_vertex.uv + delta_uv;
    const float4 object_color = BindlessSrv::sample(mat.diffuse_map, uv, mat.diffuse_map_sampler_type);

    float3 light_term = float3(0.0, 0.0, 0.0);
    for (uint i = 0; i < light_cnt; i++)
    {
        const light::PointLight point_light = scene.point_lights[i];
        light_term += PhongLight::eval_point_light(
            point_light,
            frame_data.camera_pos,
            input.coarse_vertex.world_pos,
            normal,
            object_color
        );
    }

    const float3 min_color = object_color.xyz * 0.5;

    PsOutput output = (PsOutput)0;
    output.color = float4(max(light_term, min_color), 1.0f);
    return output;
}
