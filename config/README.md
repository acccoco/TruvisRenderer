# Config

`config/` 存放由项目维护并进入 Git 的 workspace runtime 配置，不存放第三方 SDK、可执行文件或参考源码。

- `vulkan/khronos_validation_settings.txt`：Khronos validation layer 设置，由 `just cornell` 与 `just truvis` 默认启用；需要关闭时追加 `no-validation`。

第三方工具、SDK 与参考源码位于 [`external/`](../external/README.md)。工作区目录由 `TruvisPath` 提供，
资源清单和 Shader package manifest 分别位于根目录的 `resources.toml` 和 `shader-manifest.toml`。
Streamline module 自用的 runtime JSON 位于 `cxx/modules/truvixx-streamline/runtime/`。
