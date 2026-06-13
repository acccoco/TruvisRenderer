#include "api/pass/ui_blit.slangi"
#include "lib/core/bindless.slangi"

/// UI/debug blit compute 入口。
///
/// 该入口按像素写入目标 image 的偏移区域。当前保留历史调试行为：写入固定红色 marker，
/// 注释中的源 image load 表示原始 blit 意图，后续恢复采样时应同时检查 RenderGraph 状态声明。
[push_constant]
ui_blit::PushConstant g_params;

[shader("compute")]
[numthreads(ui_blit::SHADER_X, ui_blit::SHADER_Y, 1)]
void main(uint3 dispatchThreadID: SV_DispatchThreadID)
{
    if (dispatchThreadID.x > g_params.src_image_size.x ||
        dispatchThreadID.y > g_params.src_image_size.y)
    {
        // dispatch 会按线程组向上取整，多余线程必须跳过目标写入。
        return; // Out of bounds
    }

    BindlessUav::store(
        g_params.dst_image,
        dispatchThreadID.xy + g_params.offset,
        // g_params.src_image.load(dispatchThreadID.xy)
        float4(1.0, 0.0, 0.0, 1.0)
    );
}
