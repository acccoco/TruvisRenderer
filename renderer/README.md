# renderer

`renderer/` 放置构建在通用 Engine 之上的具体 Renderer、Subsystem、RenderGraph pass、shader 与
传输无关的通信契约。该层可以依赖 `engine/`，但不能依赖 Tauri、WebView 或 `app/`。

当前 `truvis-renderer` 拥有 Truvis 产品 Renderer、渲染侧 controller 和 typed ports；五个 `renderer-*`
目录承载公共 capability。`renderer/samples/*` 分别拥有 Triangle、ShaderToy 与 Cornell Renderer。
`renderer/editor/bridge` 保存传输无关的 Editor 协议，`renderer/shader` 保存 Renderer shader、ABI 与 binding。
Native CXX project 与 Rust FFI binding 统一位于 [`../cxx/`](../cxx/README.md)，不镜像 Renderer 物理分层。
