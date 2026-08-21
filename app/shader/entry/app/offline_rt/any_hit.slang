#include "lib/app/offline_rt/payload.slangi"
#include "lib/engine/scene/material_access.slangi"

/// offline RT alpha cutout any-hit。
///
/// 这里只决定候选三角形是否被忽略，不写 radiance/path 状态。透明材质的折射/反射仍由
/// closest-hit 写入 surface 后交给 raygen 材质采样处理。
[shader("anyhit")]
void trans_any(inout RtTracePayload payload, in BuiltInTriangleIntersectionAttributes attr)
{
    const uint instance_id = InstanceID();
    const uint geometry_id = GeometryIndex();
    const uint primitive_id = PrimitiveIndex();

    // RT 与同步 raycast 使用同一套 alpha cutout 规则，避免同一材质在两条路径中可见性不一致。
    if (MaterialOpacity::is_candidate_cutout(instance_id, geometry_id, primitive_id, attr.barycentrics))
    {
        IgnoreHit();
    }
}
