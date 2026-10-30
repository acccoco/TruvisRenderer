# truvis-streamline-binding

`truvis-streamline-binding` 是 Streamline / DLSS 的 Rust 侧绑定 crate。它负责三件事：

- 通过 bindgen 使用 `truvixx-streamline-capi` 暴露的稳定 C ABI。
- 用 `StreamlineRuntime` 管理进程级 `slInit` / `slShutdown` 生命周期。
- 把 Streamline 的 `logMessageCallback` 接入项目统一的 Rust `log` facade。

本 crate 只面向 Windows x64，不提供跨平台抽象。Streamline SDK 的路径参数使用
null-terminated UTF-16，Vulkan loader 也按 Windows DLL 路径处理。Rust 侧会把
`sl.interposer.dll` 的绝对路径传给 C++ wrapper；C++ 使用 `LoadLibraryW` /
`GetProcAddress` 显式解析 `slInit` / `slShutdown`，不通过链接期导入库自动加载
Streamline runtime。

## 源码边界

- `src/lib.rs`：crate 入口，只声明模块并维持对外 re-export，不放具体生命周期实现。
- `src/config.rs`：Streamline 初始化参数、默认路径推导和 feature flags。
- `src/dlss.rs`：DLSS SR/RR 的 POD 转换与 unsafe feature 入口，显式保留 raw handle 和调用顺序前置。
- `src/runtime.rs`：`StreamlineRuntime`、`StreamlineError` 和 `slInit/slShutdown` RAII 生命周期。
- `src/log_bridge.rs`：Streamline log callback 到 Rust `log` facade 的异步桥接。

## 日志链路

Streamline 日志不是由 C++ wrapper 直接输出，而是被转成 Rust 日志事件：

```text
Streamline logMessageCallback
  -> C++ sl_log_callback
  -> Rust extern "C" callback
  -> bounded queue
  -> streamline-logger thread
  -> log::debug! / log::warn! / log::error!
  -> truvis-logs / env_logger
```

可以把这条链路理解成一条传送带：C++ 负责从 Streamline 手里接过日志，Rust callback
负责快速投递，`streamline-logger` 线程才是真正写入 Rust 日志系统的一端。

如果应用入口没有初始化 `truvis_logs::TruvisLogger::init()` 或其他 `log` backend，日志事件仍会进入
`log` facade，但不会自动显示。这和项目里其他 `log::info!` / `log::warn!` 调用一致。

## C++ 侧职责

C++ wrapper 不静态导入 `sl.interposer.dll`。`truvixx_sl_init` 会先使用 Rust 传入的
`interposer_dll_path_utf16` 加载同一份 `sl.interposer.dll`，再解析 `slInit` /
`slShutdown` 并组装 `sl::Preferences`。随后 wrapper 始终向 Streamline 注册内部
`sl_log_callback`，该 callback 只做轻量工作：

- 把 `sl::LogType` 映射成稳定的 `TruvixxSlLogType`，避免 Rust 直接依赖 Streamline C++ ABI。
- 把 Rust 传入的稳定 feature flags 翻译成 Streamline SDK 的 feature id。
- 把 DLSS SR/RR 的 C++ SDK types 包装成稳定 C ABI POD，Rust 只传 Vulkan handles 和显式资源契约。
- SR options 由具体 Renderer 在 mode/resize 边界设置；RR options 包含相机矩阵，必须逐帧提交。
- RR 的 `specular_hit_distance` 使用 `kBufferTypeSpecularHitDistance`，不再提交 specular MV。
  它是 render extent 的 R32_SFLOAT 世界距离，与同帧 normal/depth 和 view/projection 矩阵配套。
- evaluate wrapper 使用同一 frame token 提交 constants、tags 并调用 `slEvaluateFeature`。Streamline
  2.14.1 的 `slAllocateResources` 固定读取 frame 0，不用于此按帧 tagging 路径；feature 由 evaluate 创建。
- SR/RR 均按 `use_linear_depth` 选择 device depth 或 linear depth tag；当前 Renderer 传 false，
  不能据此忽略 binding 调用方显式指定的深度类型。
- evaluate 返回 0 表示成功。Streamline 2.14.1 的 `sl.common/commonInterface.cpp::slEvaluateFeatureInternal`
  只在 evaluate 成功后将显存预算超限报告为 `eWarnOutOfVRAM`；C++ wrapper 将此警告归为成功，SDK
  警告仍通过日志桥输出，避免错误触发历史 reset。其它错误值原样传播，不将所有非零结果视为成功。
- 通过 Rust 传入的全局 `TruvixxSlLogCallback` 转发日志事件。
- 在 `LoadLibraryW` / `GetProcAddress` 失败时，通过同一条日志链路输出路径和 Win32 错误。

导出解析期间，尚未发布的 DLL 引用由 native RAII 管理；加载/解析诊断自身抛 C++ 异常时，
loader 返回 `eErrorExceptionHandler`，不在 catch 中再次分配诊断文本。Win32 系统错误缓冲同样
按 RAII 释放。init/shutdown 和 feature 导出也隔离 C++ 异常；这仅保证异常不穿过 Rust ABI，
不承诺捕获访问违例、恢复 SDK 状态或回滚已录制 GPU 命令。feature 输出参数仅在返回成功时消费。

C++ 侧不使用 `OutputDebugStringA` fallback。没有 Rust callback 时，日志会被丢弃。

## Rust 侧职责

Rust callback 是 FFI 边界上的快速入口，它不直接做最终日志输出。它只负责：

- 检查 C++ 传入的指针是否为空。
- 立即把 `message_utf8 + message_len` 复制成 Rust `String`，不保存 C++ 字符串指针。
- 把日志事件通过 `try_send` 放入容量为 1024 的 bounded queue。
- 用 `catch_unwind` 包住 callback，防止 Rust panic 跨过 C ABI 边界。

callback 中不要调用 SL API，不要执行文件 IO，不要直接做复杂日志格式化。原因是 SL callback
可能发生在初始化、关闭、渲染线程或 Vulkan interposer 调用栈中；这些位置都不适合被日志
系统的锁或 IO 阻塞。

Streamline feature 选择由 `StreamlineInitInfo` 负责：默认请求 DLSS SR + DLSS RR，Debug
可通过 `TRUVIS_STREAMLINE_IMGUI` 显式请求 SL ImGui 调试 UI；Release 中该环境变量会被
warning 后忽略。C++ wrapper 不根据构建类型做策略判断。
默认 Vulkan loader 路径通过 `StreamlineInitInfo::default_vulkan_loader_path()` 暴露；
这个 helper 只计算当前 executable 目录下的 `sl.interposer.dll`，不检查 DLL 是否存在。

`init` 在目录创建和安装一次性日志桥前拒绝空路径/内嵌 NUL，并将 plugin/log 目录固定为绝对路径。
native init 与后续 `vulkan_loader_path()` 使用同一路径，不再随 cwd 变化重新解释相对值；这里不做
canonicalize，也不提前替代 DLL 缺失诊断。`init_default` 将 executable 查询失败作为错误返回；
直接调用配置的 `Default` 则在该错误时 panic，不静默退到 `.`。

## drain 线程

`StreamlineLogBridge` 创建时会启动名为 `streamline-logger` 的线程。它从 queue 中取出
日志事件，并统一调用 Rust `log` facade：

```text
SL Info  -> log::debug!(target: "streamline", ...)
SL Warn  -> log::warn!(target: "streamline", ...)
SL Error -> log::error!(target: "streamline", ...)
```

最终 Rust formatter 看到的线程是 `streamline-logger`；SL 原始 callback 可能来自 init、
shutdown、渲染线程或 Vulkan interposer 调用栈，因此 callback 内只做复制和入队。

## 队列满策略

日志队列是 bounded queue，容量固定为 1024。callback 使用 `try_send`，队列满时不会等待，
而是丢弃新日志并增加 dropped counter。

drain 线程后续会输出一条 warn，说明丢弃了多少条 Streamline 日志。这个策略的目标是保护
SL/Vulkan/render 调用栈：日志可以丢，但 callback 不能因为日志系统繁忙而阻塞渲染线程。

## 线程安全与生命周期

Streamline runtime 是进程级单例，日志桥也跟随 `StreamlineRuntime` 的生命周期：

```text
StreamlineRuntime::init
  -> 创建 StreamlineLogBridge
  -> 把 callback、plugin 目录、interposer DLL 路径传给 C++
  -> C++ 显式加载 sl.interposer.dll 并调用 slInit

StreamlineRuntime::drop
  -> 先调用 truvixx_sl_shutdown()
  -> C++ 在 slShutdown 成功后 FreeLibrary；失败保留模块，均原子清空 callback
  -> Rust 向全局 sender 发送 Shutdown
  -> drain 线程 flush 已入队日志并退出
  -> join drain 线程
```

`StreamlineRuntime` 持有 `StreamlineLogBridge`，保证 Rust 日志入口活到 `slShutdown`
之后，因为 shutdown 期间 Streamline 仍可能输出日志。C++ 侧只保存函数指针，不持有 Rust
对象指针。生产路径下 Streamline 初始化失败会由 `Gfx::new` 直接 panic，当前不支持同一进程内
重复初始化 Streamline；日志 sender 因此使用一次性全局 `OnceLock`。

native 区分 DLL 已加载与 SDK 初始化成功。重复 init 在修改 callback 前拒绝；所有 feature 入口仅在
初始化成功状态可用。进入 SDK 后 init/shutdown 返回错误或抛 C++ 异常，wrapper 停止服务、清空回调并
保留 DLL 引用至进程退出；仅成功 shutdown 释放。2.14.1 的错误返回不保证日志 worker/插件已回收，
因此失败后不能继续渲染或尝试同进程重启；保留引用不是 SDK/GPU 清理完成。

callback 以原子指针读取一次再调用，避免失败清空与异步日志读取竞争。已读到的 Rust callback 地址和
OnceLock state 持续有效；receiver 退出后投递失败会丢弃日志。正常关闭按 SDK shutdown→poison pill→join
排空队列；SDK 失败时的晚到日志及 logger 自身失败不在完整 flush 保证内。原子回调不使其它 SDK API 可并发调用。

## 关键约束

- Rust feature 入口均为 `unsafe`：调用方须保证 runtime 存活和宿主串行调用；数值查询也会访问
  进程级函数表，不能由 `StreamlineRuntime: !Send + !Sync` 推导自由入口线程安全。
- Vulkan root、command buffer 和图像 handle 仍由调用方验证。图像须活到 GPU 使用完成，
  `eValidUntilEvaluate` 描述 tag 的内容有效期，不允许在 CPU evaluate 返回后立即销毁 Vulkan 对象。
- RR evaluate 当前只有 packed normal/roughness guide；配套 options 必须保持 packed，
  不把 options 中可表达 unpacked 视为已具备独立 roughness 图像通道。
- Rust callback 不能 panic 跨 FFI。
- Rust callback 不能调用 SL API。
- Rust callback 不能直接做最终 IO。
- C++ 不负责最终日志输出。
- C++ wrapper 不静态链接 `sl.interposer.lib`，只能通过 Rust 传入的绝对路径加载 SL DLL。
- SR/RR evaluate 是 opaque external command；Rust Renderer 层负责 RenderGraph resource state、
  pass 顺序、options 归属和 GPU idle 后再释放旧 feature resources。Runtime 不接触 DLSS API，
  也不为 Streamline 私有资源构造 Vulkan barrier。
- Streamline 初始化失败是启动失败，不做同进程 runtime retry。
- 业务侧统一通过 `log` facade 接收日志，具体输出格式由 `truvis-logs` 维护。
