#include "abi/renderer/render_passes/auto_exposure/mod.slangi"
[shader("compute")]
[numthreads(256, 1, 1)]
void main(uint3 id : SV_DispatchThreadID) {
    renderer::render_passes::auto_exposure::histogram[id.xy] = 0;
}
