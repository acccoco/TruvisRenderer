# Runtime Foundation

`engine/runtime/e00-foundation/` 提供 Engine 运行时与构建工具共用的基础能力。

- [`truvis-logs/`](truvis-logs/README.md) 提供日志初始化和日志文件路径工具。
- [`truvis-utils/`](truvis-utils/src/lib.rs) 提供 `ConfigUtils` 等通用工具。
- `truvis-path/` 的[路径配置](truvis-path/src/path_config.rs)以编译期确定的工作区根目录为基准，
  提供 assets、resources、`build/shader/`、`build/bindings/` 和 `.temp/` 等路径。它不负责下载资源，也不
  解析 shader package 语义。
- `truvis-shader-manifest/`：解析并校验 `shader-manifest.toml` 的 package、compiler、输出前缀
  与 binding 配置，并提供 shader artifact 路径。它依赖 `truvis-path`，但不负责执行编译。

工具 crate 位于 [`engine/tools`](../../tools/README.md)。Shader 源码和 ABI owner 位于
[`engine/shader`](../../shader/README.md)；Renderer shader 仍由 workspace 顶层的 `renderer/shader`
拥有。
