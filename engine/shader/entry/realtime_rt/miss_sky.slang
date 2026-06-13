#include "./payload.slangi"

#include "lib/core/bindless.slangi"
#include "lib/lighting/env_map.slangi"

/// 天空 miss 着色器
/// 当光线未命中任何几何体时，返回天空信息
[shader("miss")]
void sky_miss(inout RtTracePayload payload)
{
    const float2 uv = EnvMap::dir_to_uv(WorldRayDirection());
    const float3 sky_color = float3(BindlessSrv::sample_level(gpu_scene.sky, uv, gpu_scene.sky_sampler_type, 0.0).xyz) * 8.f;

    payload.hit = false;

    // miss 作为环境事件返回给 raygen；不伪装成几何表面，也不承担 path 积分职责。
    payload.surface.position = float3(0.f);
    payload.surface.object_position = float3(0.f);
    payload.surface.instance_id = 0xFFFFFFFF;
    payload.surface.forward_normal = -WorldRayDirection();
    payload.surface.origin_normal = -WorldRayDirection();
    payload.surface.object_normal = -WorldRayDirection();
    payload.surface.uv = uv;
    payload.surface.hit_t = 0.0f;

    payload.surface.base_color = sky_color;
    payload.surface.metallic = 0.f;
    payload.surface.roughness = 1.f;
    payload.surface.opaque = 1.f;
    payload.surface.ior = 1.f;

    payload.surface.emissive = sky_color;
    payload.surface.material_type = MaterialType::NO_HIT;
}
