#include "./payload.slangi"
#include "lib/scene/geometry_eval.slangi"
#include "lib/scene/scene_access.slangi"

/// 同步 raycast closest-hit。
///
/// 该 shader 只生成拾取所需的几何命中信息，不执行材质 shading。alpha cutout 已经由 any-hit
/// 过滤，因此进入这里的候选都可作为 CPU 拾取结果。
[shader("closesthit")]
void main_closest_hit(inout RayCastPayload payload, in BuiltInTriangleIntersectionAttributes attr)
{
    const uint instance_id = InstanceID();
    const uint geometry_id = GeometryIndex();
    const uint primitive_id = PrimitiveIndex();

    geometry::Geometry* geometry = SceneAccess::get_geometry(instance_id, geometry_id);
    const uint3 triangle = GeometryEval::triangle(geometry, primitive_id);
    const float3 interp_pos = GeometryEval::interp_position(geometry, triangle, attr.barycentrics);
    const float3 interp_normal = GeometryEval::interp_normal(geometry, triangle, attr.barycentrics);
    const float2 interp_uv = GeometryEval::interp_uv(geometry, triangle, attr.barycentrics);

    const float3 world_pos = mul(ObjectToWorld3x4(), float4(interp_pos, 1.f));

    scene::Instance* instance = SceneAccess::instance(instance_id);
    const float4x4 normal_matrix = transpose(instance.inv_model);
    const float3 origin_world_normal = normalize(mul(normal_matrix, float4(interp_normal, 0.f)).xyz);
    // 与 realtime RT 一致，返回朝向 ray 来向的 forward normal，便于 UI/pivot 使用稳定表面方向。
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
