#include "./payload.slangi"

/// raycast miss 只标记未命中。
///
/// 其它字段由 raygen 的初始化 sentinel 保持不变，最终写入 `RawHit` miss 结果。
[shader("miss")]
void main_miss(inout RayCastPayload payload)
{
    payload.hit = false;
}
