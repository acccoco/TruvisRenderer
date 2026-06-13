#include "api/pass/ui_blit.slangi"
#include "lib/core/bindless.slangi"


[push_constant]
ui_blit::PushConstant g_params;

[shader("compute")]
[numthreads(ui_blit::SHADER_X, ui_blit::SHADER_Y, 1)]
void main(uint3 dispatchThreadID: SV_DispatchThreadID)
{
    if (dispatchThreadID.x > g_params.src_image_size.x ||
        dispatchThreadID.y > g_params.src_image_size.y)
    {
        return; // Out of bounds
    }

    BindlessUav::store(
        g_params.dst_image,
        dispatchThreadID.xy + g_params.offset,
        // g_params.src_image.load(dispatchThreadID.xy)
        float4(1.0, 0.0, 0.0, 1.0)
    );
}
