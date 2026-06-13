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