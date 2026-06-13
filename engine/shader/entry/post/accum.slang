/// @file accum.slang
/// @brief 累积 Pass - 将单帧 RT 结果累积到 accum_image 中
///
/// 当 accum_frames == 0 时，直接复制单帧结果
/// 否则执行 lerp(old_accum, new_frame, 1/(accum_frames+1)) 混合
///
/// 该入口只表达历史累积公式，不判断相机、sky、材质或尺寸历史是否仍可复用。调用方必须在
/// `ViewAccumState` reset 后把 accum_frames 传为 0，并在 RenderGraph 中声明 accum image 的读写状态。

#include "api/pass/post_accum.slangi"
#include "lib/core/bindless.slangi"


[push_constant]
post_accum::PushConstant g_params;

[shader("compute")]
[numthreads(post_accum::SHADER_X, post_accum::SHADER_Y, 1)]
void main(uint3 dispatchThreadID: SV_DispatchThreadID)
{
    if (dispatchThreadID.x >= g_params.image_size.x ||
        dispatchThreadID.y >= g_params.image_size.y)
    {
        // dispatch 按线程组向上取整，超出有效 render extent 的线程不能写 storage image。
        return; // Out of bounds
    }

    uint2 pixel = dispatchThreadID.xy;
    
    // 读取当前帧的单帧 RT 输出
    float4 new_frame = BindlessUav::load(g_params.single_frame_input, pixel);
    
    float4 result;
    if (g_params.accum_frames == 0)
    {
        // 第一帧，直接复制
        result = new_frame;
    }
    else
    {
        // 读取累积图像中的旧值
        float4 old_accum = BindlessUav::load(g_params.accum_output, pixel);
        
        // 计算混合因子: 1 / (accum_frames + 1)
        // 例如: accum_frames=1 时，blend=0.5，新旧各占一半
        //       accum_frames=9 时，blend=0.1，新帧占 10%
        float blend = 1.0 / float(g_params.accum_frames + 1);
        
        // 线性混合
        result = lerp(old_accum, new_frame, blend);
    }
    
    // 写入累积结果
    BindlessUav::store(g_params.accum_output, pixel, result);
}
