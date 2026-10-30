# renderer

`renderer/` 放置构建在通用 Engine 之上的具体 Renderer、Subsystem、RenderGraph pass、shader 与
传输无关的通信契约。该层可以依赖 `engine/`，但不能依赖 Tauri、WebView 或 `app/`。

当前 `truvis-renderer` 拥有 Truvis 产品 Renderer 和 App Client 注入边界；五个 `renderer-*`
目录承载公共 capability。`renderer/samples/*` 分别拥有 Triangle 与 ShaderToy Renderer。
具体 Renderer 决定 ImGui 的输入消费：UI 捕获按下/滚轮意图时，坐标和按键释放仍须
更新 InputManager，避免返回 viewport 后留下旧位置或按下状态；失焦/resize 继续取消交互。
Triangle/ShaderToy 的 graphics pass 与共用 ImGui pipeline 在 resize 安全点核对 present format，
仅格式变化才重建；viewport/scissor 继续使用当前尺寸，不因普通尺寸变化重建 pipeline。
Truvis Tauri App 与 Cornell standalone App 共用完整的 `TruvisRenderer`，场景来自 App 层共享模块。
`editor/truvis-editor-bridge` 保存传输无关的 Editor 协议，`renderer/shader` 保存 Renderer shader、ABI 与 binding。
Native CXX project 与 Rust FFI binding 统一位于 [`../cxx/`](../cxx/README.md)，不镜像 Renderer 物理分层。
