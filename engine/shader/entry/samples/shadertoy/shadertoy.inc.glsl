// ShaderToy sample 的 push constant 适配层。
//
// Rust 侧每帧把鼠标、分辨率、时间和帧计数写入这里；`main.frag` 再用宏把字段映射到
// ShaderToy 常见的 iTime/iResolution/iMouse 命名，避免修改 works 下的外部示例源码。
layout(push_constant) uniform PushConstants {
    vec4 mouse;

    vec2 resolution;
    float time;
    float delta_time;

    int frame;
    float frame_rate;

    float padding_1;
    float padding_2;
} pc;