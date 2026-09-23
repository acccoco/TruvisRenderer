#include "lib/renderer/post/sdr_tone_mapper.slangi"
#include "lib/engine/core/random.slangi"
[push_constant] renderer::render_passes::sdr::PushConstant g_params;
[shader("compute")]
[numthreads(8, 8, 1)]
void main(uint3 id : SV_DispatchThreadID) {
    if (any(id.xy >= g_params.image_size)) return;
    uint2 source = min(uint2((float2(id.xy) + 0.5f) * float2(g_params.src_size) / float2(g_params.image_size)), g_params.src_size - 1);
    float4 hdr = renderer::render_passes::sdr::src_image[source];
    float3 result = hdr.rgb;
    if (g_params.radiance_channel != 0) {
        float ev = g_params.manual_ev;
        if (g_params.exposure_mode == 0) {
            ev = renderer::render_passes::sdr::exposure[uint2(0, 0)].x + g_params.auto_compensation_ev;
        }
        uint seed = Random::tea(id.y, id.x);
        float noise = g_params.dither != 0 ? (Random::rnd(seed) - 0.5f) / 255.0f : 0.0f;
        result = SdrToneMapper::map(hdr.rgb, g_params, ev, noise);
    }
    renderer::render_passes::sdr::dst_image[id.xy] = float4(result, hdr.a);
}
