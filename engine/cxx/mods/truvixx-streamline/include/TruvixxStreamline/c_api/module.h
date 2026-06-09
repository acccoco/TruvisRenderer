#pragma once

#include "TruvixxStreamline/c_api/runtime.h"

#ifdef __cplusplus
extern "C" {
#endif

// DLSS SR 当前只通过稳定 C ABI 暴露给 Rust。所有 Vulkan 对象都以整数 handle
// 传入，避免 Rust 侧直接依赖 Streamline C++ 类型或头文件 ABI。

typedef enum : uint32_t
{
    TruvixxSlDlssModeOff = 0,
    TruvixxSlDlssModeMaxPerformance = 1,
    TruvixxSlDlssModeBalanced = 2,
    TruvixxSlDlssModeMaxQuality = 3,
    TruvixxSlDlssModeUltraPerformance = 4,
    TruvixxSlDlssModeDlaa = 6,
} TruvixxSlDlssMode;

// feature support 查询结果；requirements flags 原样透传给 Rust 记录日志。
typedef struct
{
    uint32_t supported;
    uint32_t flags;
    uint32_t max_num_viewports;
    uint32_t max_num_cpu_threads;
} TruvixxSlFeatureSupport;

// DLSS SR options 的最小字段集合。pre/exposure 当前由 Rust wrapper 固定默认值，
// 后续若接入自动曝光或 alpha upscaling，再扩展这里的明确契约。
typedef struct
{
    uint32_t mode;
    uint32_t output_width;
    uint32_t output_height;
    float pre_exposure;
    float exposure_scale;
    uint32_t color_buffers_hdr;
} TruvixxSlDlssOptions;

// Streamline 根据 output size + mode 返回的推荐 render size。
typedef struct
{
    uint32_t optimal_render_width;
    uint32_t optimal_render_height;
    float optimal_sharpness;
    uint32_t render_width_min;
    uint32_t render_height_min;
    uint32_t render_width_max;
    uint32_t render_height_max;
} TruvixxSlDlssOptimalSettings;

// 非 Vulkan proxy 路径需要的 root object/queue 信息。当前主路径走 sl.interposer.dll，
// 因此这个 ABI 保留但 runtime 不主动调用。
typedef struct
{
    uint64_t instance;
    uint64_t physical_device;
    uint64_t device;
    uint32_t graphics_queue_family;
    uint32_t graphics_queue_index;
    uint32_t compute_queue_family;
    uint32_t compute_queue_index;
} TruvixxSlVulkanInfo;

// Streamline resource tag 的 Vulkan image 描述。
// layout/format/usage 必须与当前 command buffer 中的真实状态一致。
typedef struct
{
    uint64_t image;
    uint64_t memory;
    uint64_t image_view;
    uint32_t layout;
    uint32_t format;
    uint32_t width;
    uint32_t height;
    uint32_t mip_levels;
    uint32_t array_layers;
    uint32_t flags;
    uint32_t usage;
} TruvixxSlImageResource;

// Streamline common constants 的 POD 镜像。矩阵由 Rust foundation 层整理成 row-major，
// C++ wrapper 只负责逐字段搬运到 sl::Constants。
typedef struct
{
    float camera_view_to_clip[16];
    float clip_to_camera_view[16];
    float clip_to_prev_clip[16];
    float prev_clip_to_clip[16];
    float jitter_offset[2];
    float mvec_scale[2];
    float camera_pos[3];
    float camera_up[3];
    float camera_right[3];
    float camera_fwd[3];
    float camera_near;
    float camera_far;
    float camera_fov;
    float camera_aspect_ratio;
    float motion_vectors_invalid_value;
    uint32_t depth_inverted;
    uint32_t camera_motion_included;
    uint32_t motion_vectors_3d;
    uint32_t reset;
} TruvixxSlConstants;

// 一次 DLSS SR evaluate 的完整输入。当前项目固定用 viewport 0，
// depth_or_linear_depth 在 use_linear_depth=0 时 tag 为 kBufferTypeDepth。
typedef struct
{
    uint32_t frame_index;
    uint32_t viewport_id;
    uint64_t command_buffer;
    TruvixxSlConstants constants;
    TruvixxSlImageResource input_color;
    TruvixxSlImageResource output_color;
    TruvixxSlImageResource depth_or_linear_depth;
    TruvixxSlImageResource motion_vectors;
    uint32_t use_linear_depth;
} TruvixxSlDlssEvaluateDesc;

// DLSS Ray Reconstruction options 的最小字段集合。
// RR 使用与 SR 相同的 Performance Quality Mode，但作为 kFeatureDLSS_RR 独立 evaluate。
typedef struct
{
    uint32_t mode;
    uint32_t output_width;
    uint32_t output_height;
    float pre_exposure;
    float exposure_scale;
    uint32_t color_buffers_hdr;
    uint32_t normal_roughness_packed;
    float world_to_camera_view[16];
    float camera_view_to_world[16];
} TruvixxSlDlssRrOptions;

// 一次 DLSS Ray Reconstruction evaluate 的完整输入。
// normal_roughness 当前按 packed 模式 tag 为 kBufferTypeNormalRoughness。
typedef struct
{
    uint32_t frame_index;
    uint32_t viewport_id;
    uint64_t command_buffer;
    TruvixxSlConstants constants;
    TruvixxSlImageResource input_color;
    TruvixxSlImageResource output_color;
    TruvixxSlImageResource depth_or_linear_depth;
    TruvixxSlImageResource motion_vectors;
    TruvixxSlImageResource diffuse_albedo;
    TruvixxSlImageResource specular_albedo;
    TruvixxSlImageResource normal_roughness;
    TruvixxSlImageResource specular_motion_vectors;
    uint32_t use_linear_depth;
} TruvixxSlDlssRrEvaluateDesc;

// 手动 Vulkan hook 路径使用；proxy/interposer 路径不需要额外调用。
TRUVIXX_STREAMLINE_API int32_t truvixx_sl_set_vulkan_info(const TruvixxSlVulkanInfo* info);
// 查询 kFeatureDLSS support / requirements，不分配 feature resource。
TRUVIXX_STREAMLINE_API int32_t truvixx_sl_dlss_query_support(uint64_t physical_device, TruvixxSlFeatureSupport* out_support);
// 查询 kFeatureDLSS_RR support / requirements，不分配 feature resource。
TRUVIXX_STREAMLINE_API int32_t truvixx_sl_dlss_rr_query_support(uint64_t physical_device, TruvixxSlFeatureSupport* out_support);
// 查询 SR mode 对应的推荐 render extent。
TRUVIXX_STREAMLINE_API int32_t truvixx_sl_dlss_get_optimal_settings(
    const TruvixxSlDlssOptions* options,
    TruvixxSlDlssOptimalSettings* out_settings
);
// 设置指定 viewport 的 SR options；evaluate 前必须与 output resource extent 一致。
TRUVIXX_STREAMLINE_API int32_t truvixx_sl_dlss_set_options(uint32_t viewport_id, const TruvixxSlDlssOptions* options);
// 在传入的 Vulkan command buffer 上 tag resource 并执行 kFeatureDLSS。
TRUVIXX_STREAMLINE_API int32_t truvixx_sl_dlss_evaluate(const TruvixxSlDlssEvaluateDesc* desc);
// 释放指定 viewport 的 kFeatureDLSS 内部资源。
TRUVIXX_STREAMLINE_API int32_t truvixx_sl_dlss_free_resources(uint32_t viewport_id);
// 查询 RR mode 对应的推荐 render extent。
TRUVIXX_STREAMLINE_API int32_t truvixx_sl_dlss_rr_get_optimal_settings(
    const TruvixxSlDlssRrOptions* options,
    TruvixxSlDlssOptimalSettings* out_settings
);
// 设置指定 viewport 的 RR options；evaluate 前必须与 output resource extent 一致。
TRUVIXX_STREAMLINE_API int32_t truvixx_sl_dlss_rr_set_options(
    uint32_t viewport_id,
    const TruvixxSlDlssRrOptions* options
);
// 在传入的 Vulkan command buffer 上 tag resource 并执行 kFeatureDLSS_RR。
TRUVIXX_STREAMLINE_API int32_t truvixx_sl_dlss_rr_evaluate(const TruvixxSlDlssRrEvaluateDesc* desc);
// 释放指定 viewport 的 kFeatureDLSS_RR 内部资源。
TRUVIXX_STREAMLINE_API int32_t truvixx_sl_dlss_rr_free_resources(uint32_t viewport_id);

#ifdef __cplusplus
}
#endif
