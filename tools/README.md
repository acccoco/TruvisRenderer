# Tools

`tools/` 只存放外部工具资源，不放引擎内部 Rust 工具 crate。

当前主要内容：

- `slang/`：Slang 编译器、运行库、头文件和 CMake package，用于 `shader-build` 编译 shader。
- `tracy/`：Tracy profiler / capture / export 工具。
- `streamline/`：项目维护的 Streamline runtime JSON 模板，由 `truvis-cxx-build` 复制到运行目录。
- `streamline-sdk/`：Streamline SDK 下载产物，提供头文件、链接库和 runtime DLL；该目录不进入 git。
- `vulkan/khronos_validation_settings.txt`：validation layer 配置，供 `just cornell-validation` 和 `just truvis-validation` 使用。

这些资源由根目录 `resources.toml` 描述，推荐通过 `just fetch-res` 下载或刷新。
