# Config

`config/` 存放由项目维护并进入 Git 的外部工具与 runtime 配置，不存放第三方 SDK、可执行文件或参考源码。

- `streamline/`：Streamline runtime JSON 模板，由 `truvis-cxx-build` 复制到可执行文件目录。
- `vulkan/khronos_validation_settings.txt`：Khronos validation layer 设置，由 `just cornell` 与 `just truvis` 默认启用；需要关闭时追加 `no-validation`。

第三方工具、SDK 与参考源码位于 [`external/`](../external/README.md)。workspace 级路径、资源和 shader package manifest
继续由根目录的 `paths.toml`、`resources.toml` 与 `shader-packages.toml` 承载。
