#version 400
#extension GL_ARB_separate_shader_objects : enable
#extension GL_ARB_shading_language_420pack : enable

#define SHADERTOY

#include "./shadertoy.inc.glsl"

layout (location = 0) in vec2 fragCoord;

layout (location = 0) out vec4 fragColor;

// ShaderToy 兼容宏。
// 外部 works shader 仍使用原始 i* 命名；本适配层只把项目 push constant 字段映射过去。
#define iTime pc.time
#define iTimeDelta pc.delta_time
#define iResolution pc.resolution
#define iFrame pc.frame
#define iFrameRate pc.frame_rate
#define iMouse pc.mouse


// ---------------------------

// 当前选择的外部 ShaderToy 示例。works 目录下代码保持原样，不在本次中文注释补齐范围内改动。
#include "./works/chainsaw_man_power.glsl"
