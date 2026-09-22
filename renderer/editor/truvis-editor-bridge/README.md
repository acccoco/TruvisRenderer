# truvis-editor-bridge

本 crate 定义 Web Editor 与进程内 MCP Automation 共用的有界跨线程 transport。两者拥有各自的
显式 request/response 协议和独立队列；`RequestEnvelope`、oneshot reply、notification outbox 与
shutdown 语义只实现一次。

Frontend endpoint 只持有可 clone 的 request sender 和 notification receiver，RenderThread endpoint
只在 update 阶段用 `try_receive_request` 按预算消费。bridge 不依赖 `GameWorld`、Renderer、Vulkan 或
HTTP，因此协议层不能绕过 app client 直接修改场景。

Automation 的 opaque ID 仅表达 CPU scene runtime 身份，不暴露 SlotMap index、GPU slot、Vulkan handle
或资源内部路径。MCP 只使用 `AutomationRequest` / `AutomationResponse`，不会复用 Web Inspector 的
`EditorRequest` wire envelope。
