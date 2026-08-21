#include "abi/engine/binding/mod.slangi"

/// 右下角相机朝向坐标轴 gizmo。
///
/// 几何由 vertex id 直接生成，避免为固定 overlay 引入 vertex/index buffer。三轴方向通过
/// `engine::binding::per_frame_data.view` 的旋转部分从 world axis 映射到当前 camera/view 空间；位置和大小由
/// Rust 侧小 viewport/scissor 控制。
struct VsOutput
{
    float4 position : SV_Position;
    float4 color : COLOR0;
};

float2 safe_axis_dir(float2 projected, uint axis_id)
{
    const float len = length(projected);
    if (len > 0.001)
    {
        return projected / len;
    }

    const float2 fallback_dirs[3] = {
        float2(1.0, 0.0),
        float2(0.0, 1.0),
        float2(-0.7, -0.7),
    };
    return normalize(fallback_dirs[axis_id]);
}

float2 axis_point(uint axis_id, uint vertex_id)
{
    const float3 axes_ws[3] = {
        float3(1.0, 0.0, 0.0),
        float3(0.0, 1.0, 0.0),
        float3(0.0, 0.0, 1.0),
    };

    const float3 axis_vs = mul(engine::binding::per_frame_data.view, float4(axes_ws[axis_id], 0.0)).xyz;
    const float2 dir = safe_axis_dir(axis_vs.xy, axis_id);
    const float2 perp = float2(-dir.y, dir.x);

    const float projected_len = saturate(length(axis_vs.xy));
    const float axis_len = lerp(0.46, 0.74, projected_len);
    const float shaft_len = axis_len - 0.16;
    const float half_width = 0.030;
    const float arrow_width = 0.095;

    const float2 base = float2(0.0, 0.0);
    const float2 shaft_tip = dir * shaft_len;
    const float2 tip = dir * axis_len;

    const float2 shaft_vertices[6] = {
        base - perp * half_width,
        shaft_tip - perp * half_width,
        shaft_tip + perp * half_width,
        shaft_tip + perp * half_width,
        base + perp * half_width,
        base - perp * half_width,
    };
    const float2 arrow_vertices[3] = {
        tip,
        shaft_tip + perp * arrow_width,
        shaft_tip - perp * arrow_width,
    };

    return vertex_id < 6 ? shaft_vertices[vertex_id] : arrow_vertices[vertex_id - 6];
}

float4 axis_color(uint axis_id)
{
    const float4 colors[3] = {
        float4(0.95, 0.12, 0.10, 0.96),
        float4(0.18, 0.86, 0.22, 0.96),
        float4(0.20, 0.44, 1.00, 0.96),
    };
    return colors[axis_id];
}

[shader("vertex")]
VsOutput vsmain(uint vertex_id : SV_VertexID)
{
    const uint axis_id = vertex_id / 9;
    const uint axis_vertex_id = vertex_id - axis_id * 9;

    VsOutput output = (VsOutput)0;
    output.position = float4(axis_point(axis_id, axis_vertex_id), 0.0, 1.0);
    output.color = axis_color(axis_id);
    return output;
}

[shader("fragment")]
float4 psmain(VsOutput input) : SV_Target
{
    return input.color;
}
