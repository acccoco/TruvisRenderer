#pragma once
#include "TruvixxDlssNr/c_api/truvixx_dlss_nr.export.h"
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct TruvixxNrSession TruvixxNrSession;
typedef uint32_t TruvixxNrStage;
enum
{
    TruvixxNrStageRequirements = 1,
    TruvixxNrStageInit,
    TruvixxNrStageCapability,
    TruvixxNrStageCreate,
    TruvixxNrStageEvaluate,
    TruvixxNrStageRelease,
    TruvixxNrStageShutdown
};
typedef uint32_t TruvixxNrError;
enum
{
    TruvixxNrErrorNone = 0,
    TruvixxNrErrorArgument,
    TruvixxNrErrorState,
    TruvixxNrErrorThread,
    TruvixxNrErrorMissingDll,
    TruvixxNrErrorBufferTooSmall,
    TruvixxNrErrorException,
    TruvixxNrErrorDeviceContract,
    TruvixxNrErrorUnavailable
};
/// ngx_result 保留 SDK 原始返回值/能力失败码（Success=1）；wrapper 拒绝时为 0，由 error 描述原因。
typedef struct
{
    uint32_t stage;
    uint32_t ngx_result;
    uint32_t error;
} TruvixxNrStatus;
typedef struct
{
    uint32_t ngx_result;
    uint32_t value;
} TruvixxNrQuery;
typedef struct
{
    TruvixxNrQuery available, needs_updated_driver, feature_init_result, min_driver_major, min_driver_minor;
} TruvixxNrCapabilities;
typedef struct
{
    char name[256];
} TruvixxNrExtension;

/// requirements 查询只需要路径；device 查询还需要 instance/physical_device。
/// 初始化借用全部 Vulkan root 与同一 interposer 的函数指针，不加载另一个 Vulkan loader。
/// enabled_features 是调用方对实际启用状态的声明，不是物理设备支持状态。
typedef struct
{
    uint64_t instance, physical_device, device, get_instance_proc_addr, get_device_proc_addr;
    const uint16_t* dll_directory_utf16;
    const uint16_t* log_directory_utf16;
    uint32_t enabled_features;
} TruvixxNrInitDesc;
typedef uint32_t TruvixxNrDeviceFeatures;
enum
{
    TruvixxNrFeatureBufferDeviceAddress = 1u << 0,
    TruvixxNrFeatureFloat16 = 1u << 1,
    TruvixxNrFeatureStorage16 = 1u << 2,
    TruvixxNrFeatureSynchronization2 = 1u << 3,
    TruvixxNrRequiredFeatures = 15
};

/// 单 mip/layer 的 color-aspect 图像；实际 layout 必须为 GENERAL。
/// 句柄、图像内容和 view 至少存活到该次提交的 GPU 工作完成，NR 不接管所有权。
typedef struct
{
    uint64_t image, image_view;
    uint32_t format, width, height, layout;
} TruvixxNrImage;
/// 颜色/输出/遮罩匹配创建尺寸；depth/MV 同尺寸、非零，允许低于输出分辨率。
/// 颜色为 gamma 2.2 SDR；MV 为 render-pixel 单位，支持 RG16F/RG32F。
/// intensity 有限且在 [0,1]；Preset B / Style 0 / tone 0.3 / structure 0.7 / AutoMask off 固定。
typedef struct
{
    TruvixxNrImage color, depth, motion_vectors, control_mask, output;
    uint32_t reset;
    float jitter_x, jitter_y, motion_scale_x, motion_scale_y;
    float intensity;
} TruvixxNrEvaluateDesc;

/// device_extensions=0 查询 instance，=1 查询 device。*count 输入容量、输出需求数量。
/// extensions=NULL 时只返回数量；字符串复制给调用方，不跨 DLL 转移 allocation。
TRUVIXX_DLSS_NR_API TruvixxNrStatus truvixx_nr_extensions(const TruvixxNrInitDesc*, uint32_t device_extensions, TruvixxNrExtension* extensions, uint32_t* count);
/// 进程只允许一个显式 NR 会话；重复初始化直接返回 State，不重新操作全局 NGX。
/// NR DLL 必须在 Streamline 首次 NGX 初始化前已经部署在其搜索目录。
TRUVIXX_DLSS_NR_API TruvixxNrStatus truvixx_nr_init(const TruvixxNrInitDesc*, TruvixxNrSession**);
TRUVIXX_DLSS_NR_API TruvixxNrStatus truvixx_nr_capabilities(TruvixxNrSession*, TruvixxNrCapabilities*);
TRUVIXX_DLSS_NR_API TruvixxNrStatus truvixx_nr_create(TruvixxNrSession*, uint64_t command_buffer, uint32_t width, uint32_t height);
TRUVIXX_DLSS_NR_API TruvixxNrStatus truvixx_nr_evaluate(TruvixxNrSession*, uint64_t command_buffer, const TruvixxNrEvaluateDesc*);
/// 调用方先等待 GPU；释放 feature 不关闭会话，不影响其它 SR/RR feature。
TRUVIXX_DLSS_NR_API TruvixxNrStatus truvixx_nr_release(TruvixxNrSession*);
/// 所有 NR 和 SR/RR feature 已释放、GPU 已完成后调用，随后才关闭 Streamline。
/// 成功置空 *session；失败保留句柄供诊断，不在析构中隐式重试或关闭全局 NGX。
TRUVIXX_DLSS_NR_API TruvixxNrStatus truvixx_nr_shutdown(TruvixxNrSession** session);
#ifdef __cplusplus
}
#endif
