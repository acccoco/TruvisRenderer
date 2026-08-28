#include "lib/renderer/realtime_rt/payload.slangi"
#include "lib/engine/core/bindless.slangi"
#include "lib/engine/scene/geometry_eval.slangi"
#include "lib/engine/scene/material_access.slangi"
#include "lib/engine/scene/scene_access.slangi"

/// hit shader 内的采样事件分类。
///
/// CPU material class 只描述材质语义；这里把 class 与 roughness 合成为 raygen 实际需要的
/// delta / non-delta 事件。opacity 只参与后续 transmission 采样，不参与 delta 判定。
struct RtMaterialClassifier
{
    static MaterialType classify(engine::material::PbrMaterial* mat)
    {
        const float smooth_delta_roughness_threshold = 0.02f;

        if (mat.material_class == engine::material::MATERIAL_CLASS_EMISSIVE)
        {
            return MaterialType::EMISSIVE;
        }

        const bool smooth_delta = mat.roughness <= smooth_delta_roughness_threshold;
        if (mat.material_class == engine::material::MATERIAL_CLASS_TRANSMISSION)
        {
            return smooth_delta ? MaterialType::DELTA_TRANSMISSION : MaterialType::ROUGH_TRANSMISSION;
        }
        if (smooth_delta)
        {
            return MaterialType::DELTA_SPECULAR;
        }
        return MaterialType::ROUGH_SURFACE;
    }
};

[shader("closesthit")]
void main_closest_hit(inout RtTracePayload payload, in BuiltInTriangleIntersectionAttributes attr)
{
    const uint instance_id = InstanceID();
    const uint geometry_id = GeometryIndex();
    const uint primitive_id = PrimitiveIndex();

    // ========== 获取几何体数据 ==========
    engine::geometry::Geometry* geometry = SceneAccess::get_geometry(instance_id, geometry_id);
    const uint3 triangle = GeometryEval::triangle(geometry, primitive_id);
    const float3 interp_pos = GeometryEval::interp_position(geometry, triangle, attr.barycentrics);
    const float3 interp_normal = GeometryEval::interp_normal(geometry, triangle, attr.barycentrics);
    const float2 interp_uv = GeometryEval::interp_uv(geometry, triangle, attr.barycentrics);

    // 世界空间位置
    const float3 world_pos = mul(ObjectToWorld3x4(), float4(interp_pos, 1.f));

    // 世界空间法线。origin_world_normal 保留几何原始方向，forward_normal 则服务双面 shading。
    float3 origin_world_normal;
    {
        engine::scene::Instance* instance = SceneAccess::instance(instance_id);
        const float4x4 normal_matrix = transpose(instance.inv_model);
        origin_world_normal = normalize(mul(normal_matrix, float4(interp_normal, 0.f)).xyz);
    }
    // 双面材质：确保 shading normal 朝向光线来的方向。
    // GBufferA / DLSS RR 复用这个 forward normal，使 denoiser 输入与当前 path tracing 的 BRDF 法线一致。
    const float3 world_normal = faceforward(origin_world_normal, WorldRayDirection(), origin_world_normal);

    // ========== 获取材质数据 ==========
    engine::material::PbrMaterial* mat = SceneAccess::get_material(instance_id, geometry_id);

    // 获取基础颜色
    float3 base_color;
    {
        base_color = MaterialAccess::base_color(mat, interp_uv);
    }

    // closest-hit 只整理一次几何表面事件；path 积分状态由 raygen 侧状态对象持有。
    payload.hit = true;

    payload.surface.position = world_pos;
    payload.surface.object_position = interp_pos;
    payload.surface.instance_id = instance_id;
    payload.surface.geometry_id = geometry_id;
    payload.surface.primitive_id = primitive_id;
    payload.surface.forward_normal = world_normal;
    payload.surface.origin_normal = origin_world_normal;
    payload.surface.object_normal = interp_normal;
    payload.surface.uv = interp_uv;
    payload.surface.hit_t = RayTCurrent();

    payload.surface.base_color = base_color;
    payload.surface.metallic = mat.metallic;
    payload.surface.roughness = mat.roughness;
    payload.surface.opacity = mat.opacity;
    payload.surface.ior = mat.ior;

    payload.surface.emissive = mat.emissive * base_color;

    payload.surface.material_type = RtMaterialClassifier::classify(mat);
}
