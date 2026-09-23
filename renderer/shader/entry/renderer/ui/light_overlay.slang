#include "abi/renderer/render_passes/light_overlay/mod.slangi"

struct VsOutput
{
    float4 position : SV_Position;
    float4 color : COLOR0;
};

[shader("vertex")]
VsOutput vsmain(float4 position : POSITION0, float4 color : COLOR0)
{
    VsOutput output;
    output.position = position;
    output.color = color;
    return output;
}

[shader("fragment")]
float4 psmain(VsOutput input) : SV_Target { return input.color; }
