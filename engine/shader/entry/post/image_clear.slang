#include "api/pass/image_clear.slangi"
#include "lib/core/bindless.slangi"

/// storage image 清理 compute 入口。
///
/// 该入口用于 RenderGraph 中需要确定内容、但不需要保留历史的图像。它只写入目标 image，
/// 因此调用方必须用 `STORAGE_WRITE_COMPUTE` 声明资源状态。
[push_constant]
image_clear::PushConstant g_params;

[shader("compute")]
[numthreads(image_clear::SHADER_X, image_clear::SHADER_Y, 1)]
void main(uint3 dispatchThreadID: SV_DispatchThreadID)
{
    // Rust 侧 dispatch 按线程组尺寸向上取整；这里保护尾部线程不写出有效图像区域。
    if (dispatchThreadID.x >= g_params.image_size.x ||
        dispatchThreadID.y >= g_params.image_size.y)
    {
        return;
    }

    // clear_color 已经由调用方按目标图像的数值空间提供，本 pass 不做 tone mapping 或格式转换。
    BindlessUav::store(g_params.dst_image, dispatchThreadID.xy, g_params.clear_color);
}
