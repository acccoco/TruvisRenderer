#include "./payload.slangi"
#include "lib/bindless_op.slangi"

[shader("anyhit")]
void main_any_hit(inout RayCastPayload payload, in BuiltInTriangleIntersectionAttributes attr)
{
    const uint instance_id = InstanceID();
    const uint geometry_id = GeometryIndex();
    const uint primitive_id = PrimitiveIndex();

    PBRMaterial* mat = gpu_scene.get_material(instance_id, geometry_id);
    Geometry* geometry = gpu_scene.get_geometry(instance_id, geometry_id);
    const uint3 triangle = geometry.get_triangle(primitive_id);
    const float2 interp_uv = geometry.get_interp_uv(triangle, attr.barycentrics);

    float alpha = mat.opaque;
    if (bindless_srv::is_valid(mat.diffuse_map))
    {
        alpha *= bindless_srv::sample_level(mat.diffuse_map, interp_uv, mat.diffuse_map_sampler_type, 0.0).w;
    }

    if (alpha <= 0.01f)
    {
        IgnoreHit();
    }
}
