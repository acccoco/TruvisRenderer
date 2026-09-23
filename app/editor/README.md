# Web Editor

`app/editor/web` 是 Truvis 的 React/TypeScript Tauri WebView 页面。CPU `GameWorld` 仍是唯一权威场景状态；
Web 只保存可丢弃的 scene、selection 和 material 投影。

## 边界

- `renderer/editor/truvis-editor-bridge`：Rust DTO、每请求 oneshot reply，以及 Frontend/Renderer 两端的有界 channel。
- `app/editor/web`：Vite + React + TypeScript 页面；真实 backend 只通过 Tauri invoke/event 访问。
- `../truvis-app/src/editor_ipc.rs`：Tauri request、notification dispatcher、两秒 timeout 和 shutdown owner。
- `app/truvis-app/src/client/editor_controller.rs`：RenderThread Client 中的 Editor DTO 到 `GameWorld` API 和 SlotMap handle 适配器。
- `app/truvis-app/src/client/desktop_command.rs`：RenderThread Client 中的本地特权命令消费者，不属于通用 Editor DTO。

```text
Tauri WebView
  -> truvis_app::editor_ipc
       -> frontend EditorEndpoint
            <bounded in-process channels>
       -> TruvisAppClient (RenderThread)
            -> EditorController / DesktopCommandController / SceneInitializer
                 -> GameWorld
```

Bridge、Renderer 和 RenderThread Client 都禁止依赖 Tauri。`EditorIpc` 不解释领域请求；controller 不实现
`SubsystemLifecycle`。

## 协议与 IPC

Rust 协议类型定义在 `renderer/editor/truvis-editor-bridge/src/protocol/`，TypeScript 位于
`app/editor/web/src/protocol/generated/`，由 `ts-rs` 生成，禁止手工修改。`create_editor_bridge` 返回
`FrontendEndpoint` 与 `RendererEndpoint`：

- request inbox 容量为 `256`，每个 request 携带独立 oneshot reply。
- Renderer 每帧最多处理 `32` 条、最多使用 `500 μs`，只在 update 阶段访问 `GameWorld`。
- notification outbox 容量为 `64`，队列满时允许丢弃。
- `../truvis-app/capabilities/main-editor.json` 只允许 `main` WebView listen/unlisten notification event。
- 页面保留一秒 `scene_version` 与 selection 轮询，以恢复刷新或事件丢失造成的投影失效。

`Choose HDRI` 是 Tauri-only 平台动作：完整 `PathBuf` 只经过 App/Renderer 之间的私有队列，WebView
只接收文件名和 accepted/cancelled/error。accepted 不表示 decode、GPU upload 或 Alias distribution 已完成。

## Instance Inspector

Instance Inspector 的 `World Transform` 以只读 Location / Rotation / Scale 显示，Location 沿用世界单位，
Rotation 为 intrinsic XYZ Euler 角度。矩阵分解及角度转换由 `truvis-world` 完成，Web 和 app-client
不进行矩阵分解。无法可靠分解时仅显示该分组的提示，Mesh 和 Material Bindings 仍可查看。
`?mock=1` 的最后一个 instance 提供不可分解示例，其余 instance 提供正常 TRS 投影。

## Scene 列表与参数编辑

`useEditorSession` 聚合有界分页，ScenePanel 默认显示 MeshInstance、Point、Spot、Area 和唯一 Environment 条目。
`All / Lights / MeshInstance` 与名称搜索仅过滤列表，Lights 包含 HDRI，不改变渲染可见性。
Web 的 `inspectedObject` 独立于 Renderer selection；左栏不修改 Gizmo 选择，实际视口选择变化才切换 Inspector。
Instance 材质始终从当前 bindings 选择，默认首项或视口命中的 submesh。

Light Inspector 编辑位置、线性 RGB 发光值，以及 Spot 方向/锥角、Area 朝向/宽高。
字段 patch 合并 World 最新值，不会用旧详情覆盖 Gizmo 位置；失败保留草稿，迟到回包受身份和字段 revision 保护。
Area 半轴仍是权威，无法可靠分解时只禁止形状编辑。
Environment Inspector 查询当前纹理文件名与 CPU 加载状态，复用顶部 Tauri 换图动作，并编辑启用和亮度。
CPU Ready 和 command applied 均不代表 GPU 上传或画面完成。独立浏览器 `?mock=1` 提供三类灯光和环境编辑示例，文件选择只在桌面可用。

## 构建与开发

从仓库根目录执行：

```powershell
just editor-web
just editor-web-dev
```

两个命令都会先从 Rust DTO 生成 TypeScript。`editor-web` 构建 Tauri `frontendDist` 加载的生产资源；
`editor-web-dev` 启动 Vite。独立浏览器只支持 `?mock=1`，不访问真实 Renderer。

完整所有权、背压和关闭顺序见
[`docs/design/editor-boundary-and-consistency.md`](../../docs/design/editor-boundary-and-consistency.md)。
