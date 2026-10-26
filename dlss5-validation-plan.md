# DLSS 5 Neural Rendering 验证与接入方案

记录日期：2026-10-26。本文固化验证路线；实际执行状态以运行报告为准。

阶段 1 已于 2026-10-26 在当前机器重新实施并通过最小运行验收：RTX 5070 Ti、驱动 621.01、NR DLL 310.9.16.0，`Available=1`、`NeedsUpdatedDriver=0`、`FeatureInitResult=Success`；requirements 查询成功。feature 创建、首帧 reset 后连续四帧 evaluate、GPU 等待、输出回读与清理均成功，Vulkan validation error/warning 均为 0。最终验收使用与固定版本 Remix 一致的 gamma 2.2 SDR 合成输入；不代表真实场景画质。证据见 [结果报告](.temp/dlss5-validation/results.md)，复现方法见 [验证说明](.temp/dlss5-validation/README.md)。临时验证源码、构建产物与逐轮证据均位于 `.temp/dlss5-validation/`，未纳入 Git。

阶段 2 已于同日完成：交付 NR SHARED C ABI、单向 Rust binding 与 Debug/Release 部署。两种构建下 SR/RR 分别完成基线、NR 两轮各四帧与回读、NR feature 释放/重建、缺 DLL 失败后继续执行及约定顺序的最终关闭；Vulkan validation error/warning 均为 0。原始 NGX 重复初始化和零尺寸创建失败后的 SR/RR 恢复也已实测。证据与限制见 [阶段 2 报告](.temp/dlss5-validation/stage2/results.md)，复现见 [阶段 2 入口](.temp/dlss5-validation/stage2/README.md)。这些是阶段 2 的历史验证记录，不包括 Renderer。

阶段 3 已于同日完成 Renderer 配置、资源回调、图像/feature 生命周期与 ImGui 控件，native Debug/Release 和应用构建通过。
正式 Renderer 已覆盖开关/重开、强度、SR/RR、窗口尺寸、最小化恢复、Realtime/Offline、Final/Debug Image 和退出；
最终版本 Debug 连续 resize、Release RR+NR 重开及直接退出均成功，退出码为 0。
独立缺 NR DLL 环境在 NR 关闭时继续 RR 出图，显式开启后报告 MissingDll、停止出帧并按顺序清理，退出码为 101。
RR 私有图像的同步 validation 报错在从未初始化 NR 的基线同样复现；本轮未观察到 NR 新增报错、device lost 或 shutdown 错误，
不能报告 validation 全零。阶段 3 当时仅准备资源，未调用 evaluate 或改变画面。
逐项证据与验证限制见 [阶段 3 报告](.temp/dlss5-validation/stage3/results.md)。

阶段 4 已于 2026-10-27 完成执行与显示接入验收（基线 `fd039f69`）：Debug/Release 的原生、SR、RR
均串接 NR，完成连续帧、GPU 等待和输入/输出/最终 SDR 回读；强度、切换、resize、最小化恢复及失败退出已检查。
未观察到 NR 新增 validation error、device lost 或清理错误；RR 既有私有资源同步错误仍在，不能报告 validation 全零。
必要颜色编码与 reset 契约已落实；完整颜色、运动、历史稳定性、性能和显存评价仍留给后续阶段。
证据、复现方法和限制见 [阶段 4 报告](.temp/dlss5-validation/stage4/results.md) 与 [验证入口](.temp/dlss5-validation/stage4/README.md)。

历史记录：此前文档记载 RTX 4060、驱动 616.92、相同 NR DLL 的结果为明确不可用（`Available=0`、`FeatureInitResult=0xbad0000b`，日志为 `snippet is using newer GPU arch 1B0 > 190`），requirements 返回 `NotImplemented`，未执行创建/evaluate。旧报告未随当前工作区提供；本次不以该旧结论判断当前设备。

## 目标与当前基线

最终目标是在 Truvis 的 Vulkan 渲染链路中尝试启用 DLSS 5 Neural Rendering（NR），先证明目标设备可以执行，再推进正式集成。

本次讨论中已确认：

- `resources.toml` 已声明资源，`external/rtx-remix`、`external/dlss-sdk`、`external/dlss-nr-sdk` 已就位。
- 配置固定的 Remix Runtime commit 为 `0a6cd5b1ee814ec390c358e0c9c32ece8895ddc0`，对应 Remix 1.6.1。
- 基础 DLSS SDK 配置固定为 `374959484e79a640feaba44c93ac8cfb0a03f5b5`，与 Remix 1.6.1 的 DLSS submodule 引用一致；NR 扩展包为 `rtx-remix-ngx_sdk_dlnr@2`。
- 已检查到基础 NGX 链接库 `external/dlss-sdk/lib/Windows_x86_64/x64/nvsdk_ngx_d.lib` 和 NR 运行库 `external/dlss-nr-sdk/lib/Windows_x86_64/rel/nvngx_dlssnr.dll`。
- 当前项目已有 Streamline 2.14.1 SR/RR、深度、运动向量和 temporal reset 链路。
- 本次实测设备为 NVIDIA GeForce RTX 5070 Ti，驱动 621.01；后续执行仍需重新记录设备与驱动。

已验证：固定合成输入上的能力查询、feature 创建、evaluate、GPU 完成、输出基本有效性、同进程同 device 的 Streamline 共存与清理。阶段 4 已补充正式 Renderer 串接与颜色编解码回读。尚未完成系统性的真实场景画质、动态 temporal 稳定性、GPU 耗时和显存开销验证。详见各阶段报告；资源存在或构建成功均不能替代执行证据。

## 设计边界

- 保持 `app → renderer → engine` 依赖方向。正式 native 集成归属根 `cxx/`，Rust 通过明确的 C ABI 使用 SDK。
- NR 是可叠加在 SR/RR 后的独立效果，不扩展成现有“SR 或 RR 二选一”分支中的第三种模式。
- Renderer 持有 NR 配置、历史和 feature 生命周期；明确的 subsystem 持有输入输出图像；pass 只声明资源访问并录制调用。
- 正式集成中的 Vulkan 对象继续由 RenderThread 创建、使用和销毁。GPU 完成使用后才能释放或重建 feature 与图像。
- RenderGraph 管理应用资源的访问和同步；不推测 SDK 私有资源布局，不创建新的并行提交机制。
- NR 输出只进入显示链路，不回写路径追踪累积、ReSTIR 或 radiance cache；初版限于 Realtime Final，UI、描边和 gizmo 在 NR 后合成。
- 不添加兼容层或硬件支持绕过机制。失败应保留明确的阶段、返回码与日志，不把失败帧当作有效 NR 输出。

## 阶段 1：独立能力探测与最小执行

首个实施阶段仅覆盖本节。验证程序、构建产物、输入、输出和日志集中在 `.temp/dlss5-validation/`，不提前改变正式 Renderer。

1. 核对实际 checkout、SDK 头文件和 DLL，记录 commit、DLL 版本/哈希、GPU、驱动、构建及运行命令。
2. 参考 Remix 的 NGX 初始化与能力查询，建立独立 Vulkan/NGX 验证程序。复用适合的现有构建与 Vulkan 工具，状态和生命周期集中到明确的验证类型。
3. 使用项目自己的应用标识，核对 SDK 要求的 Vulkan extension、feature、设备及函数指针；不得直接复制 Remix 的产品标识。
4. 查询 `DLSSNR.Available`、`DLSSNR.NeedsUpdatedDriver`、`DLSSNR.FeatureInitResult`，以及可获得的最低驱动版本。分别保存查询 API 返回码和参数值，不能把读取参数失败解释为硬件不支持。
5. 能力允许时，调用 `NGX_VULKAN_CREATE_DLSSNR_EXT1` 创建 feature。
6. 准备契约一致的低分辨率验证输入：颜色、深度、运动向量、控制遮罩和输出图像。首帧 reset，随后执行多帧 `NGX_VULKAN_EVALUATE_DLSSNR_EXT`；输入格式、颜色域、深度和运动约定以实际 SDK 与参考实现为准。
7. 提交命令、等待 GPU 完成、回读输出，再释放 feature 和上下文。使用 Vulkan validation，保留各阶段日志。

验收与停止条件：

- 能区分初始化失败、查询失败、明确不支持、创建失败和 evaluate 失败。
- 创建/evaluate 返回成功且 GPU 执行完成，输出可回读并满足基本有效性，才算最小运行通过；合成输入上的成功不证明真实场景效果。
- 明确不支持时记录证据，停止当前设备上的执行尝试；优先转到受支持设备复验。缺 DLL、错误初始化或输入契约问题应先修正，不能直接归因于 GPU。
- 产出简短结果记录，分别列出“构建、能力、创建、evaluate、GPU 完成、输出、清理”状态及证据路径。

## 阶段 2：最小 C ABI 与 Streamline 共存

在阶段 1 证明可执行后，将必要功能整理为根 `cxx/` 下的 NR 模块及 Rust binding，封装初始化、能力查询、创建、evaluate 和释放。通过既有 `truvis_register_runtime_files` 部署 NR DLL。

在正式实施前完成架构预检，明确 NGX 与 Streamline 的初始化/关闭 owner、Vulkan handle 关系及调用串行化要求。不能假定独立 NGX 验证通过就证明与 Streamline 共存安全，也不能让两个模块无协调地管理全局生命周期。

验收：现有 SR/RR 可用；NR 初始化、执行和关闭正常；无新增生命周期或设备错误。

当前实现与实测边界：

- NR 与 Streamline 共享内部 Project ID/engine identity；NR 独占自己的参数块与 feature，不访问 Streamline 私有 NGX context。
- NR DLL 必须在 Streamline 首次 NGX 初始化前部署至其搜索目录；仅在第二次 Init 新增路径不足以发现 NR。
- 关闭 NR 效果只释放 feature。最终由单一 owner 协调：GPU idle → 全部 SR/RR/NR feature 释放 → NR 参数销毁及会话关闭 → Streamline shutdown → Vulkan root 销毁。
- 设备 feature 采用明确核对的 float16/storage16/BDA/synchronization2 集合，并为 interposer 启用 privateData，未沿用阶段 1 的全部 core features 设置。
- 正式契约见 [native README](cxx/modules/truvixx-dlss-nr/README.md) 和 [binding README](cxx/rust/bindings/truvis-dlss-nr-binding/README.md)。同一进程同时保留 SR、RR 两个活动 feature、真实场景串接及其它驱动/硬件不在本轮范围。

## 阶段 3：Renderer 状态与资源

- `TruvisRenderer` 持有 `renderer-rendering::NrSubsystem`，统一管理 requested/effective、会话、feature、图像和独立 reset。
- 默认关闭、强度 1.0；仅 Realtime Final 且无 Debug Image 时准备资源。阶段 3 强度只保存，阶段 4 已传给 evaluate；均不影响其它历史。
- update 请求中性资源回调，在尺寸同步后、prepare 前、GPU idle 边界生效；无 present target 延后。
- 阶段 3 当时使用五张独立 output extent 图像；阶段 4 已调整为三张自有图像（RGBA16F 颜色/输出、RGBA8 UNORM 全一遮罩）加两张 Realtime 同帧借用图（内部尺寸 R32F 深度、RG32F MV）。
- 关闭/进入不适用模式释放 feature/图像并保留会话；输出 extent 改变重建，内部尺寸变化只请求 NR reset。
- 创建命令通过既有 graphics immediate 提交并等待；不接入 evaluate，不改变显示链路。显式开启失败停止渲染并清理。
- 最终 GPU idle → SR/RR/NR feature → NR 图像 → NR 会话 → 显式 Streamline shutdown → Vulkan root；释放/关闭失败记录并终止。
- 初版保留开关与整体强度；模型与其他参数先固定在已验证设置。
- 覆盖开启、关闭、resize、渲染模式切换和退出。重建与释放必须满足 GPU 完成条件。

验收：资源生命周期完整，关闭 NR 时既有渲染行为保持正常，Offline 与数据调试视图不进入 NR。

## 阶段 4：NR RenderGraph pass

已实现的数据流：

```text
原生 / SR / RR HDR → 原有测光、曝光、调色、Tone Mapping
→ gamma 2.2 SDR → NR evaluate → gamma 2.2 解码、显示 dithering
→ 原 SDR target → resolve → 描边 / Gizmo / ImGui
```

- 深度/MV 直接借用同帧内部尺寸图，不重采样，不重复 import；MV 接受 RG16F/RG32F。
- intensity 有限且在 [0,1]，0 仍执行；Preset B、Style 0、tone 0.3、structure 0.7、AutoMask off。
- SDK 图像 GENERAL，三张 NR 自有图显式 import/export 稳定跨帧状态；不增加队列、线程或每帧 idle。
- SR/RR 或 NR evaluate 失败时拒绝整帧提交/present，保留原始错误并显式清理。
- reset 仅在 NR evaluate 成功且 queue submit 返回后消费；timeline 完成另行确认。
- Debug/Release 三路径每档强度有连续四帧读回。解码误差最大约 4.2e-7；强度 0 在本轮样本输入/输出相等，
  不提升为 SDK 的通用逐像素保证。系统性画质与 temporal 稳定性仍需阶段 5。

验收通过范围：正式链路执行、有效输出、资源同步、窗口显示与失败清理；RR 历史同步报错保留为基线，详见阶段 4 报告。

## 阶段 5：颜色空间与 temporal 契约

Remix 的 `RtxContext::dispatchDlssNR` 在 NR 前执行 tone mapping，之后逆转换接回 HDR 管线。固定版本的 `tonemapping_fast_tonemapping.comp.slang` 还通过 `linearToGamma` 执行 gamma 2.2 编码，逆转换 shader 对应解码；NR 输入并非线性 LDR。阶段 4 已选择在 SDR 显示链路编码/解码 gamma 2.2，不执行逆 tone mapping。阶段 5 应系统验证这一方案的颜色、运动与历史稳定性；不能直接把线性 HDR radiance 当作 NR 输入或重复显示变换。

- 明确每个颜色缓冲的数值域、传递函数、曝光处理和后续消费者。
- 覆盖相机切换、场景/材质/光照变化、resize、重新启用 NR 的历史 reset。
- 深度、MV、颜色和 reset 必须描述同一帧；成功录制、成功提交和 GPU 完成分别记录。
- 复用历史失效通知，但不把 NR、SR/RR 和路径追踪累积的历史状态混为同一个 owner。

验收：静止与运动画面没有明显的曝光跳变、错误重投影或历史污染；UI 和辅助绘制不被 NR 修改。

## 阶段 6：真实场景效果、性能与 UI

固定场景、相机、分辨率及 SR/RR 档位，对比 NR 关闭与开启。至少覆盖静止、相机运动、物体运动、高亮、透明物体和 UI 边缘，并重复开关、resize 和退出。

分别报告：

- 构建结果与实际运行结果。
- NGX 初始化、创建、evaluate 返回状态及 Vulkan validation 结果。
- 图像差异、拖影、闪烁、曝光一致性和局部外观变化。
- GPU 耗时、显存增量与测试条件；SDK 分配统计与应用图像内存分开说明。

最小链路稳定后再补模型选择、结构/色调强度和局部遮罩 UI。正式代码变化同步更新最近的模块 README 与相关 `docs/design/`；本方案不触发 `docs/summary/` 更新。

## 实施入口与参考

项目路径均相对仓库根目录：

- `docs/ARCHITECTURE.md`：分层与 owner 约束。
- `docs/design/render-configuration-and-temporal-state.md`：配置生效、reset 与资源释放。
- `docs/design/render-graph-and-data-flow.md`：资源声明和提交边界。
- `docs/design/shader-abi-and-native-integration.md`：FFI 与 shader ABI。
- `cxx/modules/truvixx-streamline/`、`cxx/rust/bindings/truvis-streamline-binding/`：现有 native 封装及部署模式。
- `renderer/truvis-renderer/src/dlss.rs`：现有 DLSS 配置和生命周期。
- `renderer/renderer-render-passes/src/post_process/dlss_rr.rs`：外部 DLSS evaluate 的 pass 参考。
- `renderer/truvis-renderer/src/truvis_renderer.rs`：主视图、SDR 与 overlay 编排。
- `external/rtx-remix/src/dxvk/rtx_render/rtx_ngx_wrapper.cpp`：NGX 能力查询和 NR 调用。
- `external/rtx-remix/src/dxvk/rtx_render/rtx_dlss_neural_rendering.cpp`：输入、参数与执行生命周期。
- `external/rtx-remix/src/dxvk/rtx_render/rtx_context.cpp`：NR 与 tone mapping 顺序。
- `external/dlss-nr-sdk/include/nvsdk_ngx_helpers_dlssnr_vk.h`：实际 Vulkan NR API。

执行时先重新阅读项目规则和上述相关实现。本文是验证路线，不能代替 SDK 契约、当前代码或真实运行证据。
