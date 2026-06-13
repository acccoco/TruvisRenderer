/// @file miss_shadow.slang
/// @brief Miss 着色器（重构版）
///
/// 处理光线未命中任何几何体时的情况（天空盒、阴影测试）

#include "./payload.slangi"

#include "lib/core/bindless.slangi"
#include "lib/lighting/env_map.slangi"

/// 阴影 miss 着色器
/// 用于阴影光线测试，如果未命中任何物体则表示该点可见光源
///
/// 阴影光线的 flag:
/// const uint ray_flags =
///     RAY_FLAG_FORCE_OPAQUE                      // 强制不透明
///     | RAY_FLAG_ACCEPT_FIRST_HIT_AND_END_SEARCH // 接受第一个命中并结束搜索
///     | RAY_FLAG_SKIP_CLOSEST_HIT_SHADER         // 跳过 closest hit shader
///     ;
[shader("miss")]
void shadow_miss(inout ShadowMissPayload payload)
{
    payload.miss = true;
}
