#include "abi/renderer/render_passes/nr_decode/mod.slangi"
#include "lib/renderer/post/display_transfer.slangi"
#include "lib/engine/core/random.slangi"
[push_constant] renderer::render_passes::nr_decode::PushConstant g_params;
[shader("compute")]
[numthreads(8, 8, 1)]
void main(uint3 id : SV_DispatchThreadID) {
    if (any(id.xy >= g_params.image_size)) return;
    float4 source = renderer::render_passes::nr_decode::src_image[id.xy];
    uint seed = Random::tea(id.y, id.x);
    float noise = g_params.dither != 0 ? (Random::rnd(seed) - 0.5f) / 255.0f : 0.0f;
    float3 linear = DisplayTransfer::decode_gamma22(source.rgb);
    renderer::render_passes::nr_decode::dst_image[id.xy] = float4(DisplayTransfer::dither(linear, noise), 1.0f);
}
