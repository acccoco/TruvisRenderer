#include "./payload.slangi"
#include "lib/scene/geometry_eval.slangi"
#include "lib/scene/material_access.slangi"
#include "lib/scene/scene_access.slangi"

/// 根据 texture 的透明度，判断光线是否应该穿过
[shader("anyhit")]
void trans_any(inout HitPayload payload, in BuiltInTriangleIntersectionAttributes attr)
{
    const uint instance_id = InstanceID();
    const uint geometry_id = GeometryIndex();
    const uint primitive_id = PrimitiveIndex();

    material::PbrMaterial* mat = SceneAccess::material(instance_id, geometry_id);
    geometry::Geometry* geometry = SceneAccess::geometry(instance_id, geometry_id);
    const uint3 triangle = GeometryEval::triangle(geometry, primitive_id);
    const float2 interp_uv = GeometryEval::interp_uv(geometry, triangle, attr.barycentrics);

    // RT 与同步 raycast 使用同一套 alpha cutout 规则，避免同一材质在两条路径中可见性不一致。
    if (MaterialOpacity::is_cutout(mat, interp_uv))
    {
        IgnoreHit();
    }
}
