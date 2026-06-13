#include "./payload.slangi"
#include "lib/scene/geometry_eval.slangi"
#include "lib/scene/material_access.slangi"
#include "lib/scene/scene_access.slangi"

/// raycast alpha cutout any-hit。
///
/// 同步 raycast 与 realtime RT 使用同一套 `MaterialOpacity` 规则，保证看起来透明的表面不会被
/// 拾取命中，也避免用户点击结果和渲染可见性不一致。
[shader("anyhit")]
void main_any_hit(inout RayCastPayload payload, in BuiltInTriangleIntersectionAttributes attr)
{
    const uint instance_id = InstanceID();
    const uint geometry_id = GeometryIndex();
    const uint primitive_id = PrimitiveIndex();

    material::PbrMaterial* mat = SceneAccess::material(instance_id, geometry_id);
    geometry::Geometry* geometry = SceneAccess::geometry(instance_id, geometry_id);
    const uint3 triangle = GeometryEval::triangle(geometry, primitive_id);
    const float2 interp_uv = GeometryEval::interp_uv(geometry, triangle, attr.barycentrics);

    if (MaterialOpacity::is_cutout(mat, interp_uv))
    {
        IgnoreHit();
    }
}
