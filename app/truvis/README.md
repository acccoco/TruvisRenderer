# truvis

`truvis` 是 Truvis Editor 的 Tauri 应用壳。它负责 Tauri WebView、原生嵌入 viewport、
`EditorIpc`、文件对话框和主线程关闭顺序；具体渲染业务由 `truvis-renderer` 提供。

## 主要职责

- `desktop`：组装 Tauri/Tao main thread、WebView、frontend ports 与 `EmbeddedWinitHost`。
- `EditorIpc`：独占 Tauri invoke、event emit、两秒 request timeout 和 notification task。
- `main` / `build.rs` / Tauri 配置：提供桌面应用启动与打包入口。
- `scripts/prepare_bistro.py`：离线准备 ORCA Bistro 三个完整场景，以 Blender 导出外部 PNG 的 glTF；
  保留源包、版权文件与 HDRI，转换 DirectX normal 的绿色通道，按源 Specular 贴图的 G/B 恢复 roughness/metallic。
  室内沿用 Wine preset 的 emission 强度量级；不导出 analytic lights，避免与自发光表面重复照明。
  玻璃映射为清透基色与源 IOR，液体保留基色 tint；原始体吸收和 nested priority 尚不支持。
  Blender/Pillow 仅属于准备阶段，应用运行时仍走现有 AssetHub/World 导入链路。
- Blender 书房场景由 `engine/e30-world/truvis-asset/scripts/export_blender_scene.py` 导出到
  `assets/scenes/office/scene.gltf` 与 `scene.json`；App 只通过 `just truvis office` 注入 manifest 路径，
  不拥有场景几何或材质表达。

## 边界约束

- 不直接访问 `World`、Vulkan、RenderGraph 或具体 render pass。
- 不解释 Editor DTO；只把请求和通知适配到 Tauri invoke/event。
- 本地文件对话框返回的路径只通过 `DesktopCommandSender` 进入 RenderThread。
- parent window 必须晚于 Renderer/Runtime/Vulkan、child HWND 与 notification task 销毁。

渲染侧职责见 [`renderer/truvis/README.md`](../../renderer/truvis/README.md)。
