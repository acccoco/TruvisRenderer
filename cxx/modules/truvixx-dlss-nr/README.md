# truvixx-dlss-nr

NR 最小 native 模块；`truvixx-dlss-nr-capi` 是 SHARED target。唯一 public ABI 是
[`include/TruvixxDlssNr/c_api/module.h`](include/TruvixxDlssNr/c_api/module.h)，不暴露 NGX C++ 类型。
当前只提供阶段 2 的会话和 feature 封装，没有 Renderer 配置、RenderGraph pass 或显示处理。

## 所有权与关闭顺序

模块拥有一个 NGX 会话、自己的参数块和至多一个 NR feature。调用方拥有 Vulkan root、command buffer、
图像及同步；所有 API 在初始化线程串行调用。初始化所用 instance/device/proc address 必须来自同一
Streamline interposer，不由 NR 模块加载第二个 Vulkan loader。

Streamline 与直接 NGX 调用不能被视为两个独立全局生命周期。一个上层 owner 必须协调：

```text
部署全部 DLL → Streamline init → interposer 创建 Vulkan root → SR/RR → NR init
关闭 NR 效果：GPU idle → NR release（保留会话）；重新开启：NR create
最终退出：GPU idle → 释放 SR/RR 与 NR feature → NR shutdown → Streamline shutdown → Vulkan root 销毁
```

`shutdown` 先销毁本模块参数，再调用 `NVSDK_NGX_VULKAN_Shutdown1(device)`；Streamline 的内部关闭随后
调用 `Shutdown1(nullptr)`。这是当前固定版本经过实测的顺序，不是 NGX 全局状态独立的保证。
重复 NR init 返回 `State`，不再次进入 SDK；已存在 feature 时拒绝 create 和 shutdown。
失败保留原始 NGX 返回码；释放失败保留对应句柄。调用方不得在失败后继续消费输出或销毁仍在使用的资源。

## 借用契约

- UTF-16 路径以零结尾，仅在调用期间借用。要求 DLL 已经部署到 Streamline 首次 NGX 初始化的搜索目录；
  二次 Init 的新增搜索路径不能保证发现 NR。
- requirements 接口复制扩展名，支持先查数量再填数组。先查询 instance requirements，创建 instance 后再查
  device requirements；启用所有返回的要求，并检查实际设备支持。由 interposer 补充 Streamline 的要求。
- 当前验证的设备 feature 集合为 `shaderFloat16`、`storageBuffer16BitAccess`、
  `uniformAndStorageBuffer16BitAccess`、`bufferDeviceAddress`、`synchronization2`；使用 Vulkan 1.3 feature
  struct 时还为 interposer 启用 `privateData`。这是一组经过验证的要求，不声称已经穷举 NR 的最小集合。
  `enabled_features` 是调用方对创建 device 时实际启用状态的声明，模块只能检查声明及关键函数入口。
- create/evaluate 只录制，不 submit、wait 或创建应用图像。command buffer 必须可录制且位于 render pass 外。
  输入写入可见性、输出读取 barrier、借用资源存活直至 GPU 完成，全部由调用方保证。
- 五张图像尺寸与 create 一致，单 mip/layer、color aspect、GENERAL layout。颜色/输出是 RGBA16F，
  深度 R32F、MV RG16F、控制遮罩 RGBA8 UNORM。输入颜色为 gamma 2.2 SDR；首帧/重建后 reset。
  输入与输出不得别名；MV scale/jitter 必须匹配同帧输入。Preset B、Style 0、intensity 1、tone 0.3、
  structure 0.7、AutoMask off 固定。真实场景的深度、运动和颜色转换仍由后续阶段验证。

能力查询返回每个参数独立的 `ngx_result/value`；总调用成功不代表 Available=1，更不能把参数读取失败
解释为硬件不支持。错误同时保留 stage、NGX 原始码和 wrapper 错误；wrapper 拒绝时 ngx_result 为 0。

## 构建与部署

`just cxx` 构建并部署 Debug/Release；Debug 链接 `nvsdk_ngx_d_dbg.lib`，Release 链接
`nvsdk_ngx_d.lib`，与项目 CRT 配置对应。NR 头文件优先于基础 SDK；版本来自既有 `resources.toml`，不在此升级。
通过 `truvis_register_runtime_files` 将 `nvngx_dlssnr.dll` 部署到相应 runtime 目录。
Project ID 和 engine version 由内部 `TruvixxUtils/ngx_identity.hpp` 与 Streamline 共享。

Rust consumer 见 [`truvis-dlss-nr-binding`](../../rust/bindings/truvis-dlss-nr-binding/README.md)。
当前机器的合成输入、共存执行与清理证据见 [阶段 2 报告](../../../.temp/dlss5-validation/stage2/results.md)。
该证据不涵盖真实 Renderer、SR/RR 输出串接到 NR、运动画质或性能。
