#include "./payload.slangi"

[shader("closesthit")]
void main_closest_hit(inout RayCastPayload payload, in BuiltInTriangleIntersectionAttributes attr)
{
    const uint instance_id = InstanceID();
    const uint geometry_id = GeometryIndex();
    const uint primitive_id = PrimitiveIndex();

    Geometry* geometry = gpu_scene.get_geometry(instance_id, geometry_id);
    const uint3 triangle = geometry.get_triangle(primitive_id);
    const float3 interp_pos = geometry.get_interp_position(triangle, attr.barycentrics);
    const float3 interp_normal = geometry.get_interp_normal(triangle, attr.barycentrics);
    const float2 interp_uv = geometry.get_interp_uv(triangle, attr.barycentrics);

    const float3 world_pos = mul(ObjectToWorld3x4(), float4(interp_pos, 1.f));

    Instance* instance = gpu_scene.get_instance(instance_id);
    const float4x4 normal_matrix = transpose(instance.inv_model);
    const float3 origin_world_normal = normalize(mul(normal_matrix, float4(interp_normal, 0.f)).xyz);
    const float3 world_normal = faceforward(origin_world_normal, WorldRayDirection(), origin_world_normal);

    payload.hit = true;
    payload.instance_slot = instance_id;
    payload.submesh_index = geometry_id;
    payload.primitive_index = primitive_id;
    payload.position_ws = world_pos;
    payload.normal_ws = world_normal;
    payload.uv = interp_uv;
    payload.hit_t = RayTCurrent();
}
