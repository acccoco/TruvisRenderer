#include "lib/engine/raycast/payload.slangi"
#include "lib/engine/scene/material_access.slangi"

/// raycast alpha cutout any-hit。
///
/// 同步 raycast 与 realtime RT 使用同一套 `MaterialOpacity` 规则，保证看起来透明的表面不会被
/// 拾取命中，也避免用户点击结果和渲染可见性不一致。
[shader("anyhit")]
void main_any_hit(inout RayCastPayload payload, in BuiltInTriangleIntersectionAttributes attr)
{
    const uint instance_id = InstanceID();
    const uint geometry_id = GeometryIndex();
    const uint primitive_id = PrimitiveIndex();

    if (MaterialOpacity::is_candidate_cutout(instance_id, geometry_id, primitive_id, attr.barycentrics))
    {
        IgnoreHit();
    }
}
