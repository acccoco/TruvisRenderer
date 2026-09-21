#include "abi/engine/binding/mod.slangi"
#include "abi/renderer/render_passes/transform_gizmo/mod.slangi"

/// 选中对象的三轴 translation gizmo。
///
/// 几何由 vertex id 生成，坐标轴在 world space 中投影；pass 使用无 depth 的
/// overlay rendering，使输入层的 CPU 命中区域与可见 gizmo 保持独立于场景 TLAS。
struct VsOutput
{
    float4 position : SV_Position;
    float4 color : COLOR0;
};

[[vk::push_constant]]
renderer::render_passes::transform_gizmo::PushConstant push_constant;

float3 axis_direction(uint axis_id)
{
    const float3 axes[3] = {
        push_constant.axis_x_ws.xyz,
        push_constant.axis_y_ws.xyz,
        push_constant.axis_z_ws.xyz,
    };
    return normalize(axes[axis_id]);
}

float3 safe_side_direction(float3 axis)
{
    float3 side = cross(engine::binding::per_frame_data.camera_forward, axis);
    if (dot(side, side) <= 0.000001)
    {
        side = cross(float3(0.0, 1.0, 0.0), axis);
    }
    if (dot(side, side) <= 0.000001)
    {
        side = cross(float3(1.0, 0.0, 0.0), axis);
    }
    return normalize(side);
}

float3 axis_point(uint axis_id, uint vertex_id)
{
    const float3 axis = axis_direction(axis_id);
    const float3 side = safe_side_direction(axis);
    const float length_ws = push_constant.axis_length_ws;
    const float arrow_length = length_ws * 0.18;
    const float shaft_length = max(length_ws - arrow_length, length_ws * 0.2);
    const float shaft_half_width = length_ws * 0.025;
    const float arrow_half_width = length_ws * 0.09;

    const float3 origin = push_constant.origin_ws.xyz;
    const float3 shaft_tip = origin + axis * shaft_length;
    const float3 tip = origin + axis * length_ws;

    const float3 shaft_vertices[6] = {
        origin - side * shaft_half_width,
        shaft_tip - side * shaft_half_width,
        shaft_tip + side * shaft_half_width,
        shaft_tip + side * shaft_half_width,
        origin + side * shaft_half_width,
        origin - side * shaft_half_width,
    };
    const float3 arrow_vertices[3] = {
        tip,
        shaft_tip + side * arrow_half_width,
        shaft_tip - side * arrow_half_width,
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
    float4 color = colors[axis_id];
    if (push_constant.active_axis != renderer::render_passes::transform_gizmo::NO_AXIS)
    {
        if (push_constant.active_axis == axis_id)
        {
            color.rgb = min(color.rgb * 1.25, float3(1.0, 1.0, 1.0));
            color.a = 1.0;
        }
        else
        {
            color.rgb *= 0.45;
            color.a = 0.72;
        }
    }
    else if (push_constant.hovered_axis == axis_id)
    {
        color.rgb = min(color.rgb * 1.25, float3(1.0, 1.0, 1.0));
        color.a = 1.0;
    }
    return color;
}

[shader("vertex")]
VsOutput vsmain(uint vertex_id : SV_VertexID)
{
    const uint axis_id = vertex_id / 9;
    const uint axis_vertex_id = vertex_id - axis_id * 9;
    const float3 world_position = axis_point(axis_id, axis_vertex_id);

    VsOutput output = (VsOutput)0;
    output.position = mul(
        engine::binding::per_frame_data.projection,
        mul(engine::binding::per_frame_data.view, float4(world_position, 1.0)));
    output.color = axis_color(axis_id);
    return output;
}

[shader("fragment")]
float4 psmain(VsOutput input) : SV_Target
{
    return input.color;
}
