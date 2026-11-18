# Runtime Utilities

`engine/runtime/e00-utils/` 保存被 Engine 运行时和构建流程共同使用的基础路径与 shader
manifest crate。它们不拥有 Vulkan、RenderGraph、asset loading 或线程生命周期。

- `truvis-path/`：读取 workspace 根目录的 `paths.toml`，提供 workspace、assets、resources、
  shader build、binding build、temporary directory 和运行时路径入口。它不负责下载资源，也不
  解析 shader package 语义。
- `truvis-shader-manifest/`：解析并校验 `shader-packages.toml` 的 package、compiler、输出前缀
  与 binding 配置，并提供 shader artifact 路径。它依赖 `truvis-path`，但不负责执行编译。

工具 crate 位于 [`engine/tools`](../../tools/README.md)。Shader 源码和 ABI owner 位于
[`engine/shader`](../../shader/README.md)；Renderer shader 仍由 workspace 顶层的 `renderer/shader`
拥有。
