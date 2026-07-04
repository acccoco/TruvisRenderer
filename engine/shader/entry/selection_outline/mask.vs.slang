#include "api/pass/selection_outline.slangi"

/// selection outline mask 的 3D asset layout 顶点入口。
///
/// 该入口只负责把选中 submesh 写入屏幕空间 mask，不输出材质、法线或 UV。
/// `instance_idx` 由 runtime selected draw loop 注入，per-frame/scene 数据通过全局 set 2 读取。
struct VsInput
{
    [[vk::location(0)]]
    float3 pos : LOCAL_POS;
};

struct VsOutput
{
    float4 pos : SV_POSITION;
};

[[vk::push_constant]]
selection_outline::MaskPushConstant push_const;

[shader("vertex")]
VsOutput main(VsInput input)
{
    scene::Instance* instance = gpu_scene.all_instances + push_const.instance_idx;

    const float4x4 mvp = mul(per_frame_data.projection, mul(per_frame_data.view, instance.model));

    VsOutput output = (VsOutput)0;
    output.pos = mul(mvp, float4(input.pos, 1.0));
    return output;
}
