#pragma once

#ifdef __cplusplus
extern "C" {
#endif

typedef union
{
    float m[16];
    struct
    {
        float m00, m10, m20, m30;
        float m01, m11, m21, m31;
        float m02, m12, m22, m32;
        float m03, m13, m23, m33;
    };
    struct
    {
        float col0[4];
        float col1[4];
        float col2[4];
        float col3[4];
    };
} TruvixxFloat4x4;

typedef union
{
    float m[9];
    struct
    {
        float m00, m10, m20;
        float m01, m11, m21;
        float m02, m12, m22;
    };
    struct
    {
        float col0[3];
        float col1[3];
        float col2[3];
    };
} TruvixxFloat3x3;

typedef union
{
    struct
    {
        float x, y, z, w;
    };
    struct
    {
        float r, g, b, a;
    };
    float v[4];
} TruvixxFloat4;

typedef union
{
    struct
    {
        float x, y, z;
    };
    float v[3];
} TruvixxFloat3;

typedef union
{
    struct
    {
        float x, y;
    };
    float v[2];
} TruvixxFloat2;

#ifdef __cplusplus
}
#endif
