#include "lib/renderer/post/auto_exposure.slangi"
[push_constant] renderer::render_passes::auto_exposure::PushConstant g_params;
groupshared uint local_histogram[256];
[shader("compute")]
[numthreads(16, 16, 1)]
void main(uint3 id : SV_DispatchThreadID, uint index : SV_GroupIndex) {
    local_histogram[index] = 0;
    GroupMemoryBarrierWithGroupSync();
    if (all(id.xy < g_params.image_size)) {
        uint bin, weight;
        if (AutoExposureMeter::sample(renderer::render_passes::auto_exposure::src_image[id.xy].rgb,
                id.xy, g_params.image_size, g_params.metering_mode, bin, weight))
            InterlockedAdd(local_histogram[bin], weight);
    }
    GroupMemoryBarrierWithGroupSync();
    InterlockedAdd(renderer::render_passes::auto_exposure::histogram[uint2(index, 0)], local_histogram[index]);
}
