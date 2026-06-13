#include "./payload.slangi"
#include "lib/scene/geometry_eval.slangi"
#include "lib/scene/material_access.slangi"
#include "lib/scene/scene_access.slangi"

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
