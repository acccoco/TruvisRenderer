# DLSS / Streamline 接入步骤

> 状态：分阶段接入，更新于 2026-05-26。阶段 0-2 的 runtime 布置、
> Streamline init/shutdown wrapper 与 Vulkan loader opt-in 入口已进入第一阶段实现；
> DLSS evaluate、resource tagging 与 RenderGraph pass 尚未接入。

本文聚焦 DLSS Super Resolution（SR）接入，不包含 Frame Generation。当前 SDK 已放在
`tools/streamline-sdk/`，版本为 Streamline 2.11.1。接入主线是：Rust 继续拥有 Vulkan
root owner、swapchain、GPU 资源和 RenderGraph；C++ 只包装 Streamline / DLSS API；Vulkan
loader 通过 `sl.interposer.dll` 进入同一套 dispatch 链。

整体顺序必须先小后大：先验证 Streamline runtime 和 Vulkan loader，再扩展 frame/view
数据，最后把 DLSS SR 作为 RenderGraph 中的 opaque external pass 接入。

## 阶段 0：整理 SDK 运行时资源

目标：明确哪些 Streamline 文件进入运行目录，避免开发期和运行期路径混乱。

当前 SDK 关键路径：

- `tools/streamline-sdk/include/`：C++ wrapper 编译所需头文件。
- `tools/streamline-sdk/lib/x64/sl.interposer.lib`：C++ wrapper 链接 SL helper API 时使用。
- `tools/streamline-sdk/bin/x64/`：生产向 runtime DLL。
- `tools/streamline-sdk/bin/x64/development/`：开发向 runtime DLL，包含 `sl.imgui.dll` 等调试组件。
- `tools/streamline-sdk/scripts/*.json`：插件配置文件。

只接入 DLSS SR 时，运行目录至少需要：

- `sl.interposer.dll`
- `sl.common.dll`
- `sl.pcl.dll`
- `sl.dlss.dll`
- `nvngx_dlss.dll`
- 相关 `sl.*.json`

第一阶段按构建 profile 选择 runtime：

- Debug：使用 `tools/streamline-sdk/bin/x64/development/`，额外复制存在的
  `WinPixEventRuntime.dll`。
- Release：使用 `tools/streamline-sdk/bin/x64/`。

实现点：

- 在 `truvis-cxx-build` 中把 DLSS SR 所需 DLL / JSON 复制到
  `target/{debug,release}` 和 `target/{debug,release}/examples`。
- 必需 DLL 只包含 `sl.interposer.dll`、`sl.common.dll`、`sl.pcl.dll`、`sl.dlss.dll`、
  `nvngx_dlss.dll`；不复制 Frame Generation / Ray Reconstruction / Reflex / NIS / ImGui DLL。
- 保留绝对路径配置能力，运行时 `slInit` 的 `Preferences::pathsToPlugins` 指向实际 DLL 目录。
- 不把整个 SDK 目录当作运行时搜索路径，只选择需要的 DLL / JSON。
- Rust 侧运行时路径和 UTF-16 path buffer 由 `truvis-path::PathUtils` 提供，避免各 binding crate
  重复实现 executable 目录和 Windows wide path 转换。

验证点：

- 运行目录可以找到 `sl.interposer.dll`、`sl.dlss.dll` 和 `nvngx_dlss.dll`。
- Streamline 日志可以写到明确目录，找不到 plugin 时能快速定位路径问题。

## 阶段 1：新增 C++ Streamline C API 模块

目标：先建立最小 C++ wrapper，不让 Rust 直接绑定 Streamline C++ ABI。

建议新增：

```text
engine/cxx/mods/truvixx-streamline/
  include/TruvixxStreamline/c_api/module.h
  c_api/module.cpp

engine/cxx/mods/truvixx-utils/
  include/TruvixxUtils/path.hpp
  include/TruvixxUtils/string.hpp

engine/cxx/truvis-streamline-binding/
  build.rs
  src/lib.rs
```

第一阶段只做生命周期 wrapper，因此 C++ 侧暂时只需要包含 `sl.h`。路径默认值、UTF-16
指针转换、UTF-8 错误文本和目录创建放在 static target `truvixx-utils` 中，并通过
`PathUtils` / `StringUtils` 静态工具 struct 聚合；Streamline wrapper 只保留进程级
init/shutdown 状态、日志回调和 `sl::Preferences` 组装。后续补 raw Vulkan handles、extent、
resource state 和 per-frame constants 时，再引入 Vulkan headers，并继续使用 `VK_NO_PROTOTYPES`
避免误调另一套 loader。

第一版 C API 只需要覆盖生命周期和诊断：

```c
truvixx_sl_init(config)
truvixx_sl_shutdown()
truvixx_sl_is_initialized()
truvixx_sl_last_error_utf8()
```

`TruvixxSlInitDesc` 同时携带 Rust 侧传入的日志 callback 和 `user_data`。C++ wrapper 始终向
Streamline 注册内部 `sl_log_callback`，该 callback 只做两件事：`eError` 更新 wrapper 的
`last_error`，所有 SL 日志事件按稳定 C ABI 转发给 Rust。最终输出统一走 Rust `log` facade，
不保留 `OutputDebugStringA` fallback。

Rust binding 内部持有一个 Streamline 日志桥：FFI callback 只复制 `message_utf8`、记录 SL
callback 所在 native thread id，并通过容量为 1024 的 bounded queue 入队；`streamline-log-drain`
线程再把 `Info/Warn/Error` 分别映射到 `debug!/warn!/error!`。队列满时丢弃新日志并累加计数，
后续由 drain 线程补一条 warn，避免 SL/Vulkan 调用栈被日志 IO 阻塞。

后续再补 DLSS SR API：

```c
truvixx_sl_dlss_get_optimal_settings(options, out_settings)
truvixx_sl_dlss_set_options(viewport, options)
truvixx_sl_dlss_evaluate(cmd, frame, resources, constants)
truvixx_sl_dlss_free_resources(viewport)
```

实现约束：

- C API 只暴露 POD，不暴露 `sl::` 类型、C++ 容器或引用。
- C++ wrapper 内部统一把 `sl::Result` 转成项目自己的错误码或文本。
- C++ wrapper 不直接实现通用路径/字符串工具；这些 helper 统一复用 `truvixx-utils` 的
  `PathUtils` / `StringUtils`。
- Rust 日志 callback 不调用 SL API，不做最终 IO，不跨 FFI 传播 panic；`StreamlineRuntime`
  在 `slShutdown` 返回后才释放日志桥，保证 callback `user_data` 生命周期覆盖 SL runtime。
- `slInit` 使用 `featuresToLoad = { sl::kFeatureDLSS }`，并设置 `renderAPI = sl::RenderAPI::eVulkan`。
- `Preferences::flags` 建议启用 `eUseFrameBasedResourceTagging`；是否启用 OTA 后续再定。

验证点：

- `just cxx` 能编译新增模块，并把 DLL / lib 复制到 Cargo target。
- Rust binding crate 能生成绑定并链接 C API DLL。
- 不创建任何 Vulkan 对象时，单独 init/shutdown 不崩溃，并能输出 SL 日志。

## 阶段 2：让 Vulkan Entry 可选择 Streamline Loader

目标：让 `ash::Entry` 从 `sl.interposer.dll` 创建，使后续 Vulkan 对象都沿同一套 SL dispatch。

当前 `GfxCore::new` 固定使用：

```rust
let vk_pf = unsafe { ash::Entry::load() }.expect("Failed to load vulkan entry");
```

第一阶段已改成 opt-in 可配置来源：

```text
VulkanEntrySource::System
VulkanEntrySource::DllPath(PathBuf) // 当前 executable 目录下的 sl.interposer.dll
```

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

实现约束：

- `slInit` 必须发生在任何 Vulkan API 调用之前。
- `GfxCore`、surface、swapchain、debug utils、device extension loader 都必须沿同一个
  `ash::Entry` 创建。
- C++ wrapper 不应独立加载普通 `vulkan-1.dll`，也不应创建第二套 Vulkan dispatch table。
- 第一版不主动调用 `slSetVulkanInfo`；只有没有走 SL `vkCreateInstance` /
  `vkCreateDevice` proxy 的手动集成才需要它。

验证点：

- 使用系统 loader 时，现有示例行为不变。
- 使用 `sl.interposer.dll` loader 时，现有渲染仍可运行。
- Streamline 日志中能看到 Vulkan interposer 和 DLSS plugin 初始化信息。

## 阶段 3：查询 Feature Requirements 并合并 Vulkan 创建需求

目标：在真正创建 Vulkan instance/device 前，知道 DLSS 对 extensions、features 和 queue 的要求。

Streamline 的 `slGetFeatureRequirements(sl::kFeatureDLSS, requirements)` 会返回：

- required buffer tags
- Vulkan instance extensions
- Vulkan device extensions
- Vulkan 1.2 / 1.3 feature names
- 可能需要的 compute / graphics queue 数量

设计选择：

- 如果确认 SL proxy 会自动处理 `vkCreateInstance` / `vkCreateDevice` 所需扩展和 feature，
  可以先以日志验证为主，不急着手动合并。
- 如果 validation 或 SL 日志显示缺少 requirements，则在 `GfxInstance` / `GfxDevice`
  创建阶段显式合并这些 extensions / features。

当前项目 `GfxDevice::basic_device_exts()` 和 `physical_device_extra_features()` 是静态列表。
DLSS 接入后，需要给它们预留外部追加能力，而不是在 DLSS 模块里反向修改 gfx 内部。

实现点：

- 新增 `GfxCreateDesc` 或类似结构，包含 app name、instance extra extensions、loader source、
  optional device extra extensions/features。
- C++ wrapper 提供查询 DLSS requirements 的 C API，并把字符串列表复制成 Rust 可消费数据。
- 第一版先打印 requirements，确认 DLSS SR 是否要求项目当前未开启的 Vulkan 特性。

验证点：

- 启用 DLSS feature 后，requirements 日志稳定。
- 若 requirements 包含当前未启用的 device extension，创建 device 前能明确报错或合并。

## 阶段 4：拆分 FrameSettings 的渲染尺寸和输出尺寸

目标：让渲染分辨率由 DLSS optimal settings 决定，而不是始终等于 swapchain extent。

当前 `FrameSettings::frame_extent` 同时承担 render extent 和 output extent 语义。DLSS SR
需要：

- `output_extent`：最终显示分辨率，通常等于 swapchain extent。
- `render_extent`：低分辨率渲染尺寸，由 `slDLSSGetOptimalSettings` 和 DLSS mode 决定。

建议演进：

```rust
pub struct FrameSettings {
    pub color_format: vk::Format,
    pub depth_format: vk::Format,
    pub render_extent: vk::Extent2D,
    pub output_extent: vk::Extent2D,
}
```

实现点：

- 保留兼容 helper，逐步替换现有 `frame_extent` 使用点。
- resize 时先更新 `output_extent`，再根据 DLSS mode 查询 `render_extent`。
- DLSS disabled 时，`render_extent == output_extent`。
- 所有 scene / RT / GBuffer / motion vector 资源使用 `render_extent`。
- DLSS output / GUI / present target 使用 `output_extent`。

验证点：

- DLSS disabled 时现有画面不变。
- 修改窗口尺寸后，FIF 资源按正确尺寸重建。
- 不同 DLSS mode 下 `render_extent` 与 `output_extent` 日志清晰。

## 阶段 5：扩展 View / Temporal 数据

目标：给 DLSS 提供稳定的 temporal constants，而不是只传当前相机矩阵。

DLSS SR 需要 per-frame constants：

- jitter offset，单位为 pixel space。
- 当前和上一帧 camera matrices。
- projection matrix 不应包含 jitter；jitter 作为单独字段提供。
- motion vector scale，按项目 motion vector 表示方式设置。
- depth 是否 inverted。
- reset history，处理相机切换、resize、模式切换、场景大跳变。

当前 `RenderView` 只有当前 view/projection/inverse 和 camera 方向。建议扩展为：

```text
RenderView
  -> current stable camera matrices
PreparedView / TemporalView
  -> current matrices without jitter
  -> previous matrices without jitter
  -> jitter offset in pixels
  -> reset_history
```

实现约束：

- 不直接把 `glam::Mat4` 当作 SL matrix memcpy；需要明确 row-major / column-major 转换。
- DLSS constants 的矩阵不包含 jitter。
- 现有累积渲染的 reset 逻辑可以作为 `reset_history` 的来源之一，但不能完全等同。

验证点：

- 输出每帧 jitter、reset history、render/output extent。
- resize、DLSS mode 切换、相机跳变时能触发 history reset。

## 阶段 6：产出 DLSS 必需资源

目标：让 RenderGraph 中有明确的 DLSS input color、depth / linear depth、motion vectors 和 output color。

DLSS SR 最小 resource tags：

- `kBufferTypeScalingInputColor`
- `kBufferTypeScalingOutputColor`
- `kBufferTypeDepth` 或 `kBufferTypeLinearDepth`
- `kBufferTypeMotionVectors`

当前资源缺口：

- `GBufferB` 包含 world position 和 linear depth，但这不一定适合直接作为
  `kBufferTypeLinearDepth`。DLSS 期望的是独立可解释的 depth / linear depth buffer。
- 当前项目尚未显式产出 motion vectors。
- 现有 `render_target` 既承担中间离屏输出，也承担 present 前 source，DLSS 后需要区分
  low-res scene color 和 final-res DLSS output。

建议 FIF resources 演进：

```text
render_extent resources:
  single_frame_rt
  accum / sdr input
  depth or linear_depth
  motion_vectors
  gbuffer_a / gbuffer_b / gbuffer_c

output_extent resources:
  dlss_output
  gui_output or final_render_target
```

实现点：

- 第一版可以选择 `kBufferTypeLinearDepth`，用单通道 image 明确存储 linear depth。
- motion vector 使用 `R16G16_SFLOAT` 或 `R32G32_SFLOAT`，先按 pixel space 或 normalized
  space 明确一种语义。
- 若 motion vector 是 pixel space，`sl::Constants::mvecScale` 使用 `{1 / render_width,
  1 / render_height}`。
- depth image view 如果来自 depth-stencil format，tag 时必须使用 depth-only aspect view。

验证点：

- 单独 debug view 显示 motion vector 和 depth。
- 静止相机时 motion vector 接近 0。
- 相机平移/旋转时 motion vector 方向与预期一致。

## 阶段 7：新增 DlssRgPass

目标：把 DLSS SR 作为 RenderGraph 中的 opaque external pass 插入，不把它伪装成项目自有 shader。

DLSS SR pass 语义：

```text
scene / denoise / tone mapping input
  -> low-res color
  -> depth or linear depth
  -> motion vectors
  -> DLSS SR external pass
       reads: low-res color, depth, motion vectors
       writes: final-res color
  -> GUI / resolve / present
```

它没有需要项目编译的 Slang / HLSL shader。执行时调用 Streamline 的 feature evaluate
接口，SL / DLSS plugin 会在当前 command buffer 位置录入自己的 GPU commands。实现上可能
包含内部 dispatch、copy、barrier 和 persistent history，不应假设它只是一个单纯的
compute shader。

`DlssRgPass` 建议结构：

```rust
pub struct DlssRgPass<'a> {
    pub streamline: &'a StreamlineRuntime,
    pub low_res_color: RgImageHandle,
    pub depth_or_linear_depth: RgImageHandle,
    pub motion_vectors: RgImageHandle,
    pub output_color: RgImageHandle,
    pub constants: DlssFrameConstants,
}
```

`setup()` 声明：

- read low-res color
- read depth / linear depth
- read motion vectors
- write output color

`execute()` 执行：

- 从 `RgPassContext` 取得 `GfxImage` / `GfxImageView`。
- 组装 C API 需要的 raw Vulkan handles、format、layout、usage、extent。
- 获取或传入 `sl::FrameToken` 对应当前 `FrameCounter::frame_id()`。
- 调用 `slSetConstants`、`slDLSSSetOptions`、resource tags、`slEvaluateFeature`。
- 如果失败，走 fallback upscale / resolve 路径，不能让整帧崩溃。

GUI / overlay 应在 DLSS output 之后叠加，避免 UI 被 temporal upscaler 处理。

验证点：

- DLSS disabled 时使用现有 resolve。
- DLSS enabled 但 unsupported 时自动 fallback。
- DLSS enabled 且 supported 时，画面输出尺寸等于 swapchain extent。

## 阶段 8：Barrier 和资源生命周期策略

目标：让 RenderGraph 负责 DLSS pass 前后的外部同步，Streamline 负责 DLSS 内部命令细节。

第一版建议保守建模：

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

资源生命周期：

- DLSS input color / motion vector 可使用 `eOnlyValidNow` 或 `eValidUntilEvaluate`。
- DLSS output 至少需要保证 evaluate 后到后续 GUI / present pass 期间有效。
- 如果未来接入 Frame Generation，depth / motion vector 的生命周期要求会更长，但当前 SR
  不按 FG 复杂度设计。
- 调用 `slFreeResources` 前，必须确保包含 pending `slEvaluateFeature` 的 command buffer 已提交并完成。

验证点：

- Vulkan validation 不报 layout / aspect / descriptor view 错误。
- resize 和 DLSS mode 切换时先 wait idle 或确保 timeline 安全，再释放旧 DLSS resources。

## 阶段 9：运行路径、回退和调试

目标：让 DLSS 成为可开关能力，而不是基础渲染路径的硬依赖。

运行策略：

- `PipelineSettings` 增加 upscale mode：`Native` / `DLSSQuality` / `DLSSBalanced` /
  `DLSSPerformance` / `DLAA`。
- DLSS unsupported、init failed、evaluate failed 时回退 `Native`。
- 回退路径仍使用现有 `resolve` / `sdr` 逻辑。
- 日志明确区分：SDK missing、interposer failed、feature unsupported、resource tag invalid、
  evaluate failed。

调试建议：

- 第一阶段启用 Streamline verbose log。
- 保留 DLSS required tags、render/output extent、jitter、mvec scale 的逐帧或按变化日志。
- 必要时复制 `sl.imgui.dll` 并研究 SL ImGui，但不作为第一版必须目标。

## 推荐最小落地切片

最小切片按以下顺序提交，避免一次同时改 loader、FFI、RenderGraph 和 temporal 数据：

1. **SDK 复制与 C++ wrapper 空壳**：只实现 init/shutdown/log，不触碰 Vulkan。
2. **Vulkan loader 注入**：`ash::Entry` 可从 `sl.interposer.dll` 创建，现有渲染不变。
3. **DLSS capability 查询**：调用 feature requirements / support 查询，只打印结果。
4. **尺寸拆分**：引入 `render_extent` / `output_extent`，DLSS off 时行为不变。
5. **Temporal 数据**：加入 jitter、previous matrices、history reset。
6. **Motion vector / linear depth**：先做可视化验证，不接 DLSS。
7. **DlssRgPass**：保守 barrier 接入 evaluate，失败自动 fallback。
8. **资源释放与 resize**：补齐 `slFreeResources`、mode 切换、shutdown 顺序。

这条路线的核心原则是：每一步都能单独验证。只要 `sl.interposer.dll` loader 阶段仍能跑现有
Truvis，后续 DLSS pass 的问题就会集中在 resource tag、constants 或 RenderGraph barrier，
不会和基础 Vulkan 初始化混在一起。
