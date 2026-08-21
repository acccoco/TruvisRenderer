/// selection outline mask 的片段入口。
///
/// mask 使用 R8_UNORM，只需要把选中 submesh 覆盖区域写成 1；后续 composite 通过邻域采样
/// 生成屏幕空间固定宽度外轮廓。
struct PsOutput
{
    [[vk::location(0)]]
    float4 mask : SV_TARGET0;
};

[shader("pixel")]
PsOutput main()
{
    PsOutput output = (PsOutput)0;
    output.mask = float4(1.0, 0.0, 0.0, 1.0);
    return output;
}
