# e00-utils

`engine/e00-utils/` 保存不依赖 Engine/Renderer 运行时代码结构的 workspace 工具。工具只通过显式配置、
文件系统输入和窄公共 API 工作，不拥有 shader ABI、RenderGraph、GPU resource 或线程生命周期。

- `truvis-fetch-res/`：按 `resources.toml` 下载和准备外部资源。
- `truvis-path/`：读取根目录 `map.toml`，提供 workspace、shader、binding、temp 等可信物理目录；不理解
  shader package、entry 或 binding schema。
- `truvis-shader-manifest/`：依赖 `truvis-path`，解析并校验 shader package、编译器、输出前缀与 binding
  配置；不检查目录和编译器是否存在。
- `truvis-shader-build/`：由 manifest 驱动的 SPIR-V 全量/增量编译与依赖校验。
- `truvis-shader-binding-codegen/`：消费 manifest 已解析的 header、include roots 与输出路径，统一执行
  bindgen、类型重命名、content hash 和 write-if-changed。

shader 源码与 ABI owner 仍分别位于 `engine/shader/`、`renderer/shader/`；工具层不定义 namespace、
allowlist 或跨 crate re-export policy。
