# DLSS / Streamline 接入讨论

> 状态：方案探索，更新于 2026-05-25。本文记录 DLSS Super Resolution
> 接入方向，当前尚未代表已落地实现。

本文聚焦 DLSS Super Resolution（SR）接入，不包含 Frame Generation。目标是让
Streamline 的 Vulkan interposer、C++ wrapper 和现有 RenderGraph 边界各自保持清晰。

## 总体判断

当前推荐方向：

- Rust 侧继续拥有 Vulkan root owner、swapchain、资源与 RenderGraph。
- C++ 侧只包装 Streamline / DLSS API，把复杂的 SL 结构体、feature option 和
  `slEvaluateFeature` 调用隐藏起来。
- Rust 通过 FFI 把 raw Vulkan handles、extent、resource state 和 per-frame constants
  传给 C++ wrapper。
- 只接入 DLSS SR，暂不引入 Frame Generation 相关 swapchain pacing / present hook 复杂度。

两边处于同一进程，Rust 和 C++ 依赖同一组 Streamline DLL 本身不是问题。需要避免的是
不同路径、不同版本或一边走 `sl.interposer.dll`、另一边绕过 interposer 直接使用普通
`vulkan-1.dll` 的混合 dispatch。

## Vulkan Loader 边界

Vulkan 下优先采用 Streamline interposer 路径：

```text
ash::Entry
  -> load sl.interposer.dll
  -> vkGetInstanceProcAddr / vkGetDeviceProcAddr come from SL
  -> create VkInstance / VkDevice / Swapchain
  -> ash instance/device/extension loaders use the same dispatch chain
```

只要 `VkInstance`、`VkDevice`、surface/swapchain 和后续 extension loader 都从这条
`Entry -> Instance -> Device` 链路创建，普通 Vulkan 调用通常不需要额外处理。SL 的
interposer 会转发绝大多数 Vulkan API，只拦截它需要观察或接管的路径。

约束：

- `GfxCore` 创建 `ash::Entry` 时必须能选择 `sl.interposer.dll`，不能后续再混用普通
  `ash::Entry::load()`。
- C++ wrapper 不应独立加载另一套 Vulkan loader，也不应创建第二套 Vulkan dispatch table。
- 若没有让 SL interposer 观察 instance/device 创建流程，才需要走更手动的
  `slSetVulkanInfo` 等路径。
- SL 初始化和 feature requirement 查询需要发生在创建相关 Vulkan 对象之前或最早可用阶段，
  以便追加所需 instance/device extensions 与 features。

## C++ Wrapper 对 Vulkan 的依赖

C++ wrapper 建议包含 Vulkan headers，但不负责加载 Vulkan loader：

```cpp
#define VK_NO_PROTOTYPES
#include <vulkan/vulkan.h>

#include <sl.h>
#include <sl_dlss.h>
#include <sl_helpers_vk.h>
```

`VK_NO_PROTOTYPES` 让 C++ 侧只使用 Vulkan 类型、枚举和结构体，避免隐式依赖
`vulkan-1.lib` 或误调另一套 loader。C++ wrapper 的职责是把 Rust 传入的
`VkImage`、`VkImageView`、`VkCommandBuffer`、`VkImageLayout` 等 raw values 打包为
Streamline resource tag 和 DLSS options。

## DLSS SR Pass 语义

DLSS SR 在 RenderGraph 里不应被视为本项目自有 shader pass。它更像一个
opaque external pass：

```text
scene / denoise / tone mapping input
  -> low-res color
  -> depth
  -> motion vectors
  -> DLSS SR external pass
       reads: low-res color, depth, motion vectors
       writes: final-res color
  -> GUI / resolve / present
```

它没有需要项目编译的 Slang / HLSL shader。执行时调用 Streamline 的 feature evaluate
接口，SL / DLSS plugin 会在当前 command buffer 位置录入自己的 GPU commands。实现上
可能包含内部 dispatch、copy、barrier 和 persistent history，不应假设它只是一个单纯的
compute shader。

## RenderGraph 集成形态

建议新增 `DlssRgPass`，保持与现有 `RgPass` 模型一致：

- `setup()` 声明输入输出图像依赖。
- `execute()` 从 `RgPassContext` 取得物理 image / image view，再调用 C++ wrapper。
- pass 添加顺序决定 DLSS 在帧内的位置，不让 RenderGraph 做拓扑重排。

最小依赖：

- input color：低分辨率 scene color，通常不包含 GUI。
- output color：最终显示分辨率的 DLSS 输出。
- depth：DLSS 需要的 depth 或 linear depth 语义，具体格式和 tag 需要与 SL 约定一致。
- motion vectors：低分辨率 motion vector，当前项目尚未显式产出。
- per-frame constants：jitter、render extent、output extent、camera matrices、reset history 等。

GUI / overlay 更适合在 DLSS output 之后叠加，避免 UI 被 temporal upscaler 处理。

## Barrier 策略

RenderGraph 应负责 DLSS pass 前后的外部可见同步；Streamline 负责 DLSS 内部命令细节。
第一阶段建议保守建模：

```rust
const DLSS_READ: RgImageState = RgImageState::new(
    vk::PipelineStageFlags2::ALL_COMMANDS,
    vk::AccessFlags2::MEMORY_READ,
    vk::ImageLayout::GENERAL,
);

const DLSS_WRITE: RgImageState = RgImageState::new(
    vk::PipelineStageFlags2::ALL_COMMANDS,
    vk::AccessFlags2::MEMORY_READ | vk::AccessFlags2::MEMORY_WRITE,
    vk::ImageLayout::GENERAL,
);
```

这比把 DLSS 当成普通 `COMPUTE_SHADER + SHADER_STORAGE_*` 更稳妥，因为 DLSS 的内部实现
不是项目可见契约。后续如果 SL 文档或 validation 明确允许更窄的 state，再收紧 stage /
access。

## 当前项目缺口

接入 DLSS SR 前，需要先明确以下基础能力：

- `FrameSettings` 需要区分 `render_extent` 和 `output_extent`。当前 `frame_extent` 同时承担
  渲染尺寸和输出尺寸语义。
- FIF resources 需要低分辨率 scene color、motion vector、DLSS output 等资源。
- `RenderView` 需要扩展 temporal 数据：jitter、previous view/projection、history reset。
- 现有 GBufferB 包含 world position 和 linear depth，但 DLSS 所需 depth tag 是否直接复用
  需要单独确认。
- RenderGraph 当前只管理 image barrier；如果后续 DLSS 需要 buffer 资源参与 tag，还需要扩展
  buffer resource 声明。

## 建议顺序

1. 先把 `GfxCore` 的 Vulkan entry 来源配置化，并验证所有 Vulkan 对象都从同一
   `sl.interposer.dll` dispatch 链路创建。
2. 建立 C++ Streamline wrapper 的最小生命周期：init、query requirements、shutdown。
3. 扩展 frame/view 概念，拆分 render/output extent，加入 jitter 和 previous matrices。
4. 让主渲染路径产出 motion vectors，并明确 depth 输入格式。
5. 新增 `DlssRgPass`，先用保守 `GENERAL + ALL_COMMANDS` barrier 接入。
6. 把 GUI / overlay 放到 DLSS output 之后，再进入 swapchain resolve / present。

