#include "lib/engine/scene/material_access.slangi"
#include "lib/renderer/raster/phong.slangi"
#include "abi/renderer/render_passes/raster/mod.slangi"
#include "lib/engine/core/bindless.slangi"
#include "lib/engine/lighting/phong.slangi"
#include "lib/engine/scene/scene_access.slangi"

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
renderer::render_passes::raster::PushConstants push_const;

[shader("pixel")]
PsOutput main(PsInput input)
{
    float3 normal = normalize(input.coarse_vertex.frag_normal);

    engine::material::PbrMaterial* mat = SceneAccess::get_material(push_const.instance_idx, push_const.submesh_idx);

    const uint light_cnt = engine::binding::gpu_scene.point_light_count;

    const float4 object_color = MaterialAccess::base_color_rgba(
        mat, input.coarse_vertex.uv[engine::material::TEXTURE_BASE_COLOR]);
    if (mat.coverage_mode == engine::material::COVERAGE_ALPHA_MASK && object_color.w < mat.alpha_cutoff) discard;
    const engine::material::MaterialTexture normal_slot = mat.textures[engine::material::TEXTURE_NORMAL];
    if (BindlessSrv::is_valid(normal_slot.image))
    {
        const float2 normal_uv = MaterialAccess::transform_uv(normal_slot, input.coarse_vertex.uv[engine::material::TEXTURE_NORMAL]);
        float4 tangent = input.coarse_vertex.tangent;
        if (dot(tangent.xyz, tangent.xyz) < 1e-12)
        {
            tangent = MaterialAccess::tangent_from_derivatives(normal,
                ddx(input.coarse_vertex.world_pos), ddy(input.coarse_vertex.world_pos), ddx(normal_uv), ddy(normal_uv));
        }
        normal = MaterialAccess::normal_from_sample(normal, tangent, MaterialAccess::sample(normal_slot,
            input.coarse_vertex.uv[engine::material::TEXTURE_NORMAL], float4(0.5, 0.5, 1, 1)).xyz, mat.normal_scale);
    }
    const float3 emission = mat.emissive * MaterialAccess::sample(mat.textures[engine::material::TEXTURE_EMISSIVE],
        input.coarse_vertex.uv[engine::material::TEXTURE_EMISSIVE], float4(1)).xyz;
    const float4 mr = MaterialAccess::sample(mat.textures[engine::material::TEXTURE_METALLIC_ROUGHNESS],
        input.coarse_vertex.uv[engine::material::TEXTURE_METALLIC_ROUGHNESS], float4(1));
    const float metallic = mat.metallic * mr.b;
    const float roughness = mat.roughness * mr.g;

    float3 light_term = float3(0.0, 0.0, 0.0);
    for (uint i = 0; i < light_cnt; i++)
    {
        const engine::light::PointLight point_light = engine::binding::gpu_scene.point_lights[i];
        light_term += PhongLight::eval_point_light(
            point_light,
            engine::binding::per_frame_data.camera_pos,
            input.coarse_vertex.world_pos,
            normal,
            object_color,
            metallic,
            roughness
        );
    }

    const float3 min_color = object_color.xyz * 0.5;

    PsOutput output = (PsOutput)0;
    output.color = float4(max(light_term, min_color) + emission, 1.0f);
    return output;
}
