# Truvis Agent Integration

## 第一阶段边界

`truvis-app` 在同一进程内创建一个独立 current-thread Tokio HTTP 线程，使用 RMCP 3.4.0 的
stateless Streamable HTTP service 挂载单一 `/mcp` endpoint。HTTP handler 只负责参数 schema、固定 Host
校验、向 Automation endpoint 投递请求、等待 oneshot 和构造 MCP structured result；它不持有
`GameWorld`、RenderWorld、Renderer 或 Vulkan 引用。

依赖方向保持 `app -> renderer -> engine`。Automation bridge 位于
`renderer/editor/truvis-editor-bridge`，与 Web Editor 共用有界 channel 实现，但 `AutomationRequest` /
`AutomationResponse` 与 `EditorRequest` / `EditorResponse` 显式分离。两条 request queue 独立，agent 洪峰
不会挤压 Web Editor。

## 所有权与时序

- HTTP thread 拥有 MCP service、固定 loopback listener 和 cancellation token。
- RenderThread 拥有 `TruvisAppClient`、`WorldAutomationController` 和 `GameWorld`。
- `GameWorld` 是 CPU scene 唯一业务 authority；`RenderWorld` 仍由 renderer prepare 从 CPU world 派生。
- Automation controller 每帧同时限制请求数和 wall-clock 时间，不等待 HTTP、WebView 或外部 agent。
- 请求入队成功只表示 channel 接受；oneshot response 表示 RenderThread 已完成该次 CPU query/mutation。
- `scene_version` 是 CPU scene 语义版本；asset ready、GPU visible 和最终像素完成需要后续阶段的独立状态。

第一阶段支持 scene summary、对象分页、instance/material 查询，transform/material mutation，以及 scene
import/status。opaque ID 只编码 session 内 CPU runtime identity；不暴露 SlotMap index、GPU slot、Vulkan
handle 或内部路径结构。写请求可携带 `expected_scene_version`，版本冲突在 mutation 前拒绝。

版本检查与 mutation 在同一次 RenderThread 请求处理中完成；携带同一旧版本的两个写请求，
若第一个实际改变 scene version，第二个必须返回 Conflict。缺省版本表示允许对当前状态写入，
不提供旧草稿冲突检测。预算只在请求之间检查，不能限制单次同步任务的耗时；丢弃 reply
或断开调用方等待不撤销已经入队的命令。

import accepted 仅表示获得 CPU 导入身份；Ready 表示导入数据和资源身份就绪，不会自动创建
live instance。第一阶段 Automation 尚无实例化导入结果的命令。不存在或 generation 不匹配的
import ID 返回 StaleObject，真实加载失败返回 Failed 与错误文本。import record 保留到 World
销毁；释放策略需要同时定义查询和重复实例化的生命周期，不能按一次状态读取自动回收。

## HTTP 安全与协议

固定地址是 `127.0.0.1:8765/mcp`，不使用 token 或其他身份认证；访问控制只依赖 loopback 地址和固定
Host 校验。此接口仅供本地原生客户端使用：无 Origin 请求可进入，任何携带 Origin 的请求
（含 `null`）均由 RMCP 拒绝；这不构成本地进程身份认证。支持 MCP `2025-06-18`、`2025-11-25` 和 `2026-07-28` 的 stateless Streamable HTTP：
2025 版本通过 `initialize` 协商，2026 版本可以使用 `server/discover` 和现代 request metadata；标准
`MCP-Protocol-Version` header 仍由 RMCP 校验。端口占用会直接阻止 Truvis 启动，用户需要释放端口后
重新启动；不实现旧 HTTP+SSE、REST fallback、兼容字段、自动换端口、CORS 或远程 OAuth。本地其他
进程可以访问该 endpoint。协议版本只影响 MCP envelope 和生命周期，不影响 Automation wire DTO。

## Shutdown

桌面关闭先标记 `shutting_down` 并取消 HTTP listener 的接收；随后关闭 RenderThread，使 Automation
receiver drop，未处理 oneshot 在进程内得到 renderer shutdown 错误；再 join HTTP thread，最后停止
Editor notification task 并销毁 Tauri parent window。HTTP 停止接收后只给在途连接有限的排空窗口，
当前为两秒；超时后退出专属 runtime，释放尚未完成的连接任务，不能依赖客户端补齐请求体。
正常排空期间尽量发送 reply；超时或断连时不保证错误已到达客户端，也不撤销已入队/已执行的 CPU
mutation。该限制只约束 HTTP 异步排空，不是 RenderThread/GPU 整体关闭耗时保证。

Renderer settings、camera、capture/readback 和 Lua 不属于本阶段。后续 renderer-owned lane 必须经过
RenderGraph/GPU completion；Lua 只能作为外部脚本 runner 调用稳定的 Automation API，不能取得 GameWorld、
Vulkan、RenderGraph、process、network 或任意 Rust callback。
