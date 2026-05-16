#include "./payload.slangi"
#include "lib/bindless_op.slangi"

/// 根据 texture 的透明度，判断光线是否应该穿过
[shader("anyhit")]
void trans_any(inout HitPayload payload, in BuiltInTriangleIntersectionAttributes attr)
{
    const uint instance_id = InstanceID();
    const uint geometry_id = GeometryIndex();
    const uint primitive_id = PrimitiveIndex();

    PBRMaterial* mat = gpu_scene.get_material(instance_id, geometry_id);
    Geometry* geometry = gpu_scene.get_geometry(instance_id, geometry_id);
    const uint3 triangle = geometry.get_triangle(primitive_id);
    const float2 interp_uv = geometry.get_interp_uv(triangle, attr.barycentrics);

    // 当前命中位置 diffuse 贴图是透的，直接忽略该命中
    const float4 base_color = bindless_srv::sample_level(mat.diffuse_map, interp_uv, mat.diffuse_map_sampler_type, 0.0);
    if (base_color.w == 0.f)
    {
        IgnoreHit();
    }
}
