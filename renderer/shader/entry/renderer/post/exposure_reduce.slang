#include "lib/renderer/post/auto_exposure.slangi"
[push_constant] renderer::render_passes::auto_exposure::PushConstant g_params;
[shader("compute")]
[numthreads(1, 1, 1)]
void main(uint3 id : SV_DispatchThreadID) {
    float4 previous = renderer::render_passes::auto_exposure::exposure[uint2(0, 0)];
    renderer::render_passes::auto_exposure::exposure[uint2(0, 0)] = AutoExposureMeter::reduce(g_params, previous);
}
