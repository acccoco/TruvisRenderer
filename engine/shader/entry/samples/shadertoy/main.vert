#version 400
#extension GL_ARB_separate_shader_objects : enable
#extension GL_ARB_shading_language_420pack : enable

#include "./shadertoy.inc.glsl"

// 定义全屏矩形的 6 个顶点（两个三角形）
const vec3 positions[6] = vec3[](
    vec3(-1.0, -1.0, 0.0),  // 左下
    vec3( 1.0, -1.0, 0.0),  // 右下
    vec3(-1.0,  1.0, 0.0),  // 左上
    vec3(-1.0,  1.0, 0.0),  // 左上
    vec3( 1.0, -1.0, 0.0),  // 右下
    vec3( 1.0,  1.0, 0.0)   // 右上
);

layout (location = 0) out vec2 fragCoord;

void main() {
    vec3 pos = positions[gl_VertexIndex];
    // NDC 坐标 (-1, -1) 到 (1, 1)，由于 viewport Y 轴翻转，Y=-1 在顶部，Y=1 在底部
    // shadertoy 坐标系是 left-bottom(0, 0), right-top(resolution)
    fragCoord = (pos.xy * 0.5 + 0.5) * pc.resolution;
    gl_Position = vec4(pos, 1.0);
}
