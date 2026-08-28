# renderer

`renderer/` 放置构建在通用 Engine 之上的具体 Renderer、Subsystem、RenderGraph pass、shader 与
传输无关的通信契约。该层可以依赖 `engine/`，但不能依赖 Tauri、WebView 或 `app/`。

当前 `truvis-renderer` 拥有 Truvis 产品 Renderer 与渲染侧 Editor controller；五个 `renderer-*`
目录承载公共 capability。三个
`renderer/samples/*` library 分别拥有 Triangle、ShaderToy 与 Cornell Renderer；shader 会在后续迁移提交中继续收敛到本目录。
