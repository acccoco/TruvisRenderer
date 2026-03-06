/// 同步射线检测 Compute Shader
///
/// 使用 RayQuery 对 TLAS 进行射线追踪，返回命中信息（位置、法线、材质等）。
/// 通过 push constants 传入 buffer device address，避免额外 descriptor set 创建。

#include "share/global_binding_sets.slangi"
#include "share/raycast.slangi"

struct RayCastPushConstants
{
    uint ray_count;
    uint _pad0;
    uint64_t input_buffer_addr;
    uint64_t output_buffer_addr;
};

[[vk::push_constant]]
RayCastPushConstants push_const;

[[vk::binding(0, 3)]]
RaytracingAccelerationStructure tlas;

[shader("compute")]
[numthreads(64, 1, 1)]
void main_raycast(uint3 dispatch_thread_id: SV_DispatchThreadID)
{
    uint ray_idx = dispatch_thread_id.x;
    if (ray_idx >= push_const.ray_count)
    {
        return;
    }

    RayCastInput* inputs = (RayCastInput*)push_const.input_buffer_addr;
    RayCastHit* outputs = (RayCastHit*)push_const.output_buffer_addr;

    RayCastInput input = inputs[ray_idx];

    RayDesc ray;
    ray.Origin = input.origin;
    ray.Direction = normalize(input.direction);
    ray.TMin = input.t_min;
    ray.TMax = input.t_max;

    RayQuery<RAY_FLAG_NONE> rq;
    rq.TraceRayInline(
        tlas,
        RAY_FLAG_NONE,
        0xFF,
        ray
    );

    while (rq.Proceed())
    {
        if (rq.CandidateType() == CANDIDATE_NON_OPAQUE_TRIANGLE)
        {
            rq.CommitNonOpaqueTriangleHit();
        }
    }

    RayCastHit result;
    result.hit = 0;
    result.position = float3(0, 0, 0);
    result.normal = float3(0, 0, 0);
    result.distance = -1.0;
    result.instance_id = 0;
    result.primitive_id = 0;
    result.barycentrics = float2(0, 0);
    result.base_color = float4(0, 0, 0, 0);
    result.metallic = 0;
    result.roughness = 0;
    result._pad = float2(0, 0);

    if (rq.CommittedStatus() == COMMITTED_TRIANGLE_HIT)
    {
        result.hit = 1;
        result.distance = rq.CommittedRayT();
        result.instance_id = rq.CommittedInstanceID();
        result.primitive_id = rq.CommittedPrimitiveIndex();
        result.barycentrics = rq.CommittedTriangleBarycentrics();

        uint geometry_index = rq.CommittedGeometryIndex();

        // 获取 object-to-world 变换矩阵
        float3x4 o2w = rq.CommittedObjectToWorld3x4();
        float3x4 w2o = rq.CommittedWorldToObject3x4();

        // 通过 gpu_scene 获取几何体数据并插值顶点属性
        Geometry* geom = gpu_scene.get_geometry(result.instance_id, geometry_index);
        uint3 tri = geom->get_triangle(result.primitive_id);

        float3 local_pos = geom->get_interp_position(tri, result.barycentrics);
        float3 local_normal = geom->get_interp_normal(tri, result.barycentrics);

        // 变换到世界空间
        result.position = mul(o2w, float4(local_pos, 1.0));
        // 法线使用 transpose(inverse(model)) 变换，等价于 transpose(w2o) 的 3x3 部分
        float3 world_normal;
        world_normal.x = w2o[0][0] * local_normal.x + w2o[1][0] * local_normal.y + w2o[2][0] * local_normal.z;
        world_normal.y = w2o[0][1] * local_normal.x + w2o[1][1] * local_normal.y + w2o[2][1] * local_normal.z;
        world_normal.z = w2o[0][2] * local_normal.x + w2o[1][2] * local_normal.y + w2o[2][2] * local_normal.z;
        result.normal = normalize(world_normal);

        // 读取材质
        PBRMaterial* mat = gpu_scene.get_material(result.instance_id, geometry_index);
        result.base_color = float4(mat->base_color, mat->opaque);
        result.metallic = mat->metallic;
        result.roughness = mat->roughness;
    }

    outputs[ray_idx] = result;
}
