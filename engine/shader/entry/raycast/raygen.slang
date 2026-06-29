#include "./payload.slangi"

/// 初始化 miss 状态。
///
/// sentinel 统一使用 `0xFFFFFFFF`，与 `raycast::RawHit` 的 miss 表示保持一致，便于 CPU readback
/// 直接判断字段是否有效。
RayCastPayload init_payload()
{
    RayCastPayload payload = {};
    payload.hit = false;
    payload.instance_slot = 0xFFFFFFFFu;
    payload.submesh_index = 0xFFFFFFFFu;
    payload.primitive_index = 0xFFFFFFFFu;
    payload.position_ws = float3(0.f);
    payload.normal_ws = float3(0.f);
    payload.uv = float2(0.f);
    payload.hit_t = 0.f;
    return payload;
}

raycast::RawHit make_miss()
{
    raycast::RawHit raw = {};
    raw.hit = 0;
    raw.instance_slot = 0xFFFFFFFFu;
    raw.submesh_index = 0xFFFFFFFFu;
    raw.primitive_index = 0xFFFFFFFFu;
    raw.position_ws = float3(0.f);
    raw.hit_t = 0.f;
    raw.normal_ws = float3(0.f);
    raw.uv = float2(0.f);
    return raw;
}

/// 将 closest-hit payload 压平成 readback ABI。
///
/// 这里不做 instance slot -> CPU handle 转换；runtime 在 fence 等待后读取 raw hit，再结合
/// `RenderInstanceManager` 的 raycast 反查快照做还原。
raycast::RawHit make_hit(const RayCastPayload payload)
{
    raycast::RawHit raw = {};
    raw.hit = 1;
    raw.instance_slot = payload.instance_slot;
    raw.submesh_index = payload.submesh_index;
    raw.primitive_index = payload.primitive_index;
    raw.position_ws = payload.position_ws;
    raw.hit_t = payload.hit_t;
    raw.normal_ws = payload.normal_ws;
    raw.uv = payload.uv;
    return raw;
}

[shader("raygeneration")]
void main_ray_gen()
{
    const uint ray_idx = DispatchRaysIndex().x;
    if (ray_idx >= push_const.ray_count)
    {
        // SBT dispatch 可以按对齐后的 ray 数发起，多余 invocation 不写输出 buffer。
        return;
    }

    const raycast::Ray input_ray = raycast::rays[ray_idx];

    RayDesc ray;
    // 输入 ray 已经是 world space；shader 只归一化方向，保留 CPU 侧指定的 t_min/t_max。
    ray.Origin = input_ray.origin_ws;
    ray.Direction = normalize(input_ray.direction_ws);
    ray.TMin = input_ray.t_min;
    ray.TMax = input_ray.t_max;

    RayCastPayload payload = init_payload();
    TraceRay(
        raycast::raycast_tlas,
        RAY_FLAG_NONE,
        0xFF,
        0,
        0,
        0,
        ray,
        payload
    );

    raycast::raw_hits[ray_idx] = payload.hit ? make_hit(payload) : make_miss();
}
