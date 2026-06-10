#include "./payload.slangi"

#include "lib/bindless_op.slangi"
#include "lib/env_map.slangi"

/// 天空 miss 着色器
/// 当光线未命中任何几何体时，返回天空信息
[shader("miss")]
void sky_miss(inout HitPayload payload)
{
    const float2 uv = dir_to_env_uv(WorldRayDirection());
    const float3 sky_color = float3(bindless_srv::sample_level(gpu_scene.sky, uv, gpu_scene.sky_sampler_type, 0.0).xyz) * 8.f;

    payload.hit = false;

    // 填充天空信息作为"虚拟命中"
    payload.info.position = float3(0.f);
    payload.info.forward_normal = -WorldRayDirection();
    payload.info.origin_normal = -WorldRayDirection();
    payload.info.object_normal = -WorldRayDirection();
    payload.info.uv = uv;

    payload.info.base_color = sky_color;
    payload.info.metallic = 0.f;
    payload.info.roughness = 1.f;
    payload.info.opaque = 1.f;
    payload.info.ior = 1.f;

    payload.info.emissive = sky_color;
    payload.info.material_type = MaterialType::NO_HIT;
}
