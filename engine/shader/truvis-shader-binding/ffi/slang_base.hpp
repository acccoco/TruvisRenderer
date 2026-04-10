#pragma once

typedef unsigned char uint8_t;
typedef unsigned short uint16_t;
typedef unsigned int uint;
typedef unsigned long long int uint64_t;

typedef signed char int8_t;
typedef signed short int16_t;
typedef signed int int32_t;
typedef signed long long int64_t;

struct float2 {
  float x, y;
};

struct float3 {
  float x, y, z;
};

struct float4 {
  float x, y, z, w;
  float4(float x, float y, float z, float w);
};

struct float4x4 {
  float4 col0;
  float4 col1;
  float4 col2;
  float4 col3;
};

struct int2 {
  int x, y;
};

struct int3 {
  int x, y, z;
};

struct int4 {
  int x, y, z, w;
};

struct uint2 {
  uint x, y;
};

struct uint3 {
  uint x, y, z;
};

struct uint4 {
  uint x, y, z, w;
};
