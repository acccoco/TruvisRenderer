#include "./payload.slangi"
#include "lib/core/bindless.slangi"
#include "lib/scene/geometry_eval.slangi"
#include "lib/scene/material_access.slangi"
#include "lib/scene/scene_access.slangi"

struct RtMaterialClassifier
{
    static MaterialType classify(material::PbrMaterial* mat)
    {
        if (mat.emissive.x > 0.f)
        {
            return MaterialType::EMISSIVE;
        }
        if (mat.opaque < 1.f)
        {
            return MaterialType::TRANSPARENT;
        }
        if (mat.roughness < 0.05f)
        {
            return MaterialType::SPECULAR;
        }
        return MaterialType::DIFFUSE;
    }
};

[shader("closesthit")]
void main_closest_hit(inout HitPayload payload, in BuiltInTriangleIntersectionAttributes attr)
{
    const uint instance_id = InstanceID();
    const uint geometry_id = GeometryIndex();
    const uint primitive_id = PrimitiveIndex();

    // ========== 获取几何体数据 ==========
    geometry::Geometry* geometry = SceneAccess::geometry(instance_id, geometry_id);
    const uint3 triangle = GeometryEval::triangle(geometry, primitive_id);
    const float3 interp_pos = GeometryEval::interp_position(geometry, triangle, attr.barycentrics);
    const float3 interp_normal = GeometryEval::interp_normal(geometry, triangle, attr.barycentrics);
    const float2 interp_uv = GeometryEval::interp_uv(geometry, triangle, attr.barycentrics);

    // 世界空间位置
    const float3 world_pos = mul(ObjectToWorld3x4(), float4(interp_pos, 1.f));

    // 世界空间法线。origin_world_normal 保留几何原始方向，forward_normal 则服务双面 shading。
    float3 origin_world_normal;
    {
        scene::Instance* instance = SceneAccess::instance(instance_id);
        const float4x4 normal_matrix = transpose(instance.inv_model);
        origin_world_normal = normalize(mul(normal_matrix, float4(interp_normal, 0.f)).xyz);
    }
    // 双面材质：确保 shading normal 朝向光线来的方向。
    // GBufferA / DLSS RR 复用这个 forward normal，使 denoiser 输入与当前 path tracing 的 BRDF 法线一致。
    const float3 world_normal = faceforward(origin_world_normal, WorldRayDirection(), origin_world_normal);

    // ========== 获取材质数据 ==========
    material::PbrMaterial* mat = SceneAccess::material(instance_id, geometry_id);

    // 获取基础颜色
    float3 base_color;
    {
        base_color = MaterialAccess::base_color(mat, interp_uv);
    }

    // ========== 填充命中信息 ==========
    payload.hit = true;

    // 几何信息
    payload.info.position = world_pos;
    payload.info.object_position = interp_pos;
    payload.info.instance_id = instance_id;
    payload.info.forward_normal = world_normal;
    payload.info.origin_normal = origin_world_normal;
    payload.info.object_normal = interp_normal;
    payload.info.uv = interp_uv;
    payload.info.hit_t = RayTCurrent();  // 线性深度

    // 材质参数
    payload.info.base_color = base_color;
    payload.info.metallic = mat.metallic;
    payload.info.roughness = mat.roughness;
    payload.info.opaque = mat.opaque;
    payload.info.ior = 1.5f; // 默认折射率，后续可从材质中读取

    // 自发光
    payload.info.emissive = mat.emissive * base_color;

    // 材质类型
    payload.info.material_type = RtMaterialClassifier::classify(mat);
}
