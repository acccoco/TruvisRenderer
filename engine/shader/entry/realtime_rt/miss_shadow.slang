/// @file miss_shadow.slang
/// @brief 阴影 miss 着色器。
///
/// 处理阴影光线未命中任何几何体时的情况。

#include "./payload.slangi"

#include "lib/core/bindless.slangi"
#include "lib/lighting/env_map.slangi"

/// 阴影 miss 着色器。
/// 用于阴影光线测试，如果未命中任何物体则表示该点可见光源。
///
/// 当前主直接光照使用 RayQuery inline shadow test，这个 miss group 保留为兼容/实验入口。
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
