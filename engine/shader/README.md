确保在当前 workspace 下添加了 slang 的 search path:

```json
{
  "slang.additionalSearchPaths": [
    "D:\\code\\Render-Rust-vk-Truvis\\engine\\shader"
  ],
  "slang.searchInAllWorkspaceDirectories": false
}
```

# Truvis 着色器管线 - AI 编程指南

## 架构概览

这是一个 **Vulkan 光线追踪渲染器** 的着色器模块，使用 **Slang** 作为主要着色语言。

### 目录结构

| 目录                | 用途                                        |
|-------------------|-------------------------------------------|
| `include/`        | 共享头文件（`.slangi`）—— 结构体、工具函数、全局绑定          |
| `include/pass/`   | 各渲染通道的 PushConstants 和特定绑定                |
| `include/sample/` | 采样和随机数工具（`random.slangi`、`sample.slangi`） |
| `src/`            | 按通道组织的着色器源文件（`.slang`）                    |
| `build/`          | 编译后的 SPIR-V 输出                            |

## 核心模式

### 1. 描述符集布局（三层全局绑定）

定义于 [global_binding_sets.slangi](include/global_binding_sets.slangi)：

```slang
// set 0: 全局采样器
[[vk::binding(0, 0)]] SamplerState global_samplers[];

// set 1: 无绑定资源（需要 NonUniformResourceIndex）
[[vk::binding(0, 1)]] Sampler2D<float4> bindless_textures[];
[[vk::binding(1, 1)]] RWTexture2D<float4> bindless_uavs[];
[[vk::binding(2, 1)]] Texture2D<float4> bindless_srvs[];

// set 2: 每帧数据
[[vk::binding(0, 2)]] ConstantBuffer<PerFrameData> per_frame_data;
[[vk::binding(1, 2)]] ConstantBuffer<GPUScene> gpu_scene;
```

### 2. GPU 指针宏（跨语言兼容）

[ptr.slangi](include/ptr.slangi) 定义了 `PTR()` 宏，在 Slang 中展开为指针，在 C++ FFI 中展开为 `uint64_t`：

```slang
#ifdef __SLANG__
    #define PTR(T, ident) T* ident
#else
    #define PTR(T, ident) uint64_t ident
#endif

// 使用示例（scene.slangi）
struct GPUScene {
    PTR(PBRMaterial, all_mats);
    PTR(Geometry, all_geometries);
    // ...
};
```

### 3. Handle 类型（无绑定索引）

定义于 [bindless.slangi](include/bindless.slangi)：

- `TextureHandle` — 纹理+采样器（Sampler2D）
- `SrvHandle` — 只读纹理（Texture2D）
- `UavHandle` — 可读写纹理（RWTexture2D）

访问操作封装在 [bindless_op.slangi](include/bindless_op.slangi)：

```slang
bindless_srv::sample(handle, uv, sampler_type);  // SrvHandle
bindless_texture::sample(handle, uv);            // TextureHandle
bindless_uav::load(handle, coord);               // UavHandle
```

### 4. 通道结构规范

每个渲染通道遵循：

- **头文件**：`include/pass/<pass>.slangi` — 定义 `namespace <pass>` 包含 `PushConstants` 和专用绑定
- **着色器**：`src/<pass>/<pass>.slang` — 实现入口点

示例（[rt.slangi](include/pass/rt.slangi)）：

```slang
namespace rt {
    #define RT_SET_NUM GLOBAL_SETS_COUNT  // = 3，光追专用绑定集

    [[vk::binding(0, RT_SET_NUM)]] RaytracingAccelerationStructure rt_tlas;
    [[vk::binding(1, RT_SET_NUM)]] RWTexture2D<float4> rt_color;

    struct PushConstants { uint spp; uint spp_idx; uint channel; };
};
```

## 关键结构体

| 结构体            | 文件                                             | 用途                       |
|----------------|------------------------------------------------|--------------------------|
| `PerFrameData` | [frame_data.slangi](include/frame_data.slangi) | 相机矩阵、时间、分辨率、累积帧数         |
| `GPUScene`     | [scene.slangi](include/scene.slangi)           | 场景图：材质/几何体/实例的 GPU 指针    |
| `Instance`     | [scene.slangi](include/scene.slangi)           | 模型矩阵 + 几何体/材质间接索引        |
| `Geometry`     | [geometry.slangi](include/geometry.slangi)     | 顶点缓冲区指针 + 插值辅助函数         |
| `PBRMaterial`  | [material.slangi](include/material.slangi)     | PBR 参数 + 漫反射/法线贴图 Handle |

## 着色器约定

### 命名规则

- Slang 类型：`float3`、`float4x4`、`uint2`（非 GLSL 风格）
- 文件扩展名：`.slang`（着色器）、`.slangi`（头文件）
- 入口点：`main_ray_gen`、`main_closest_hit`、`vsmain`、`psmain`

### 常用头文件依赖顺序

```slang
#include "pass/rt.slangi"       // 通道专用（自动包含 global_binding_sets）
#include "pbr.slangi"           // PBR BRDF
#include "sample/random.slangi" // Random::tea(), Random::rnd()
#include "sample/sample.slangi" // Sample::get_cos_hemisphere_sample()
#include "bindless_op.slangi"   // 纹理访问操作
```

### 光线追踪 Payload

[rt.slang](src/rt/rt.slang) 使用的 payload 结构：

```slang
struct HitPayload {
    float3 radiance;    // 累积辐射度
    float3 weight;      // BRDF * cos / pdf
    bool done;          // 终止标志
    float3 ray_origin;  // 下条光线起点
    float3 ray_dir;     // 下条光线方向
    uint random_seed;   // 随机种子状态
};
```

## 开发工作流

### VS Code 配置

已配置 [.vscode/settings.json](.vscode/settings.json)：

```json
{
  "slang.additionalSearchPaths": [
    "E:\\...\\shader\\include"
  ]
}
```

### 添加新渲染通道

1. 创建 `include/pass/<pass>.slangi` — 定义命名空间和 PushConstants
2. 创建 `src/<pass>/<pass>.slang` — 实现着色器入口点
3. 编译后输出至 `build/<pass>/`

### 修改共享结构

1. 更新 `.slangi` 头文件
2. 如有 Rust FFI 绑定，同步更新 C++ FFI 头文件
3. 重新编译依赖该结构的所有着色器
