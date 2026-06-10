#include "./payload.slangi"
#include "lib/bindless_op.slangi"

/// 确定材质类型
/// @param mat 材质指针
/// @return 材质类型
MaterialType determine_material_type(PBRMaterial* mat)
{
    // 自发光材质
    if (mat.emissive.x > 0.f)
    {
        return MaterialType::EMISSIVE;
    }
    // 透明材质
    if (mat.opaque < 1.f)
    {
        return MaterialType::TRANSPARENT;
    }
    // 镜面材质（低粗糙度视为 delta）
    if (mat.roughness < 0.05f)
    {
        return MaterialType::SPECULAR;
    }
    // 默认漫反射
    return MaterialType::DIFFUSE;
}

[shader("closesthit")]
void main_closest_hit(inout HitPayload payload, in BuiltInTriangleIntersectionAttributes attr)
{
    const uint instance_id = InstanceID();
    const uint geometry_id = GeometryIndex();
    const uint primitive_id = PrimitiveIndex();

    // ========== 获取几何体数据 ==========
    Geometry* geometry = gpu_scene.get_geometry(instance_id, geometry_id);
    const uint3 triangle = geometry.get_triangle(primitive_id);
    const float3 interp_pos = geometry.get_interp_position(triangle, attr.barycentrics);
    const float3 interp_normal = geometry.get_interp_normal(triangle, attr.barycentrics);
    const float2 interp_uv = geometry.get_interp_uv(triangle, attr.barycentrics);

    // 世界空间位置
    const float3 world_pos = mul(ObjectToWorld3x4(), float4(interp_pos, 1.f));

    // 世界空间法线。origin_world_normal 保留几何原始方向，forward_normal 则服务双面 shading。
    float3 origin_world_normal;
    {
        Instance* instance = gpu_scene.get_instance(instance_id);
        const float4x4 normal_matrix = transpose(instance.inv_model);
        origin_world_normal = normalize(mul(normal_matrix, float4(interp_normal, 0.f)).xyz);
    }
    // 双面材质：确保 shading normal 朝向光线来的方向。
    // GBufferA / DLSS RR 复用这个 forward normal，使 denoiser 输入与当前 path tracing 的 BRDF 法线一致。
    const float3 world_normal = faceforward(origin_world_normal, WorldRayDirection(), origin_world_normal);

    // ========== 获取材质数据 ==========
    PBRMaterial* mat = gpu_scene.get_material(instance_id, geometry_id);

    // 获取基础颜色
    float3 base_color;
    {
        if (bindless_srv::is_valid(mat.diffuse_map))
        {
            base_color = bindless_srv::sample_level(mat.diffuse_map, interp_uv, mat.diffuse_map_sampler_type, 0.0).xyz;
        }
        else
        {
            base_color = mat.base_color;
        }
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
    payload.info.material_type = determine_material_type(mat);
}
