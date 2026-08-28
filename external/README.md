# External

`external/` 只存放下载或检出的第三方工具、SDK 与参考源码，不放引擎内部 Rust 工具 crate，也不承载项目维护的配置。

当前主要内容：

- `slang/`：Slang 编译器、运行库、头文件和 CMake package，用于 `shader-build` 编译 shader。
- `tracy/`：Tracy profiler / capture / export 工具。
- `streamline-sdk/`：Streamline SDK 下载产物，提供头文件、链接库和 runtime DLL；该目录不进入 git。
- `rtx-gi/`：RTXGI / SHARC 参考源码 checkout，包含递归 submodule，仅作为对照实现和文档参考；该目录不进入 git。
- `rtx-di/`：RTXDI 参考源码 checkout，包含递归 submodule，仅作为对照实现和文档参考；该目录不进入 git。
- `openpbr/`：OpenPBR 参考源码 checkout，仅用于材质模型实现与文档对照；该目录不进入 git。

这些资源由根目录 `resources.toml` 描述，推荐通过 `just fetch-res` 下载或刷新。Git resource 使用 HTTP(S)
URL，并保留 `.git` 与 submodule 元数据，方便后续确认参考源码版本。网络受限时可在运行前设置
`HTTP_PROXY` / `HTTPS_PROXY` / `NO_PROXY` 或对应小写环境变量；`fetch_res` 会继承这些环境变量，不把代理地址写入配置文件。

项目维护的 Streamline runtime JSON 与 Vulkan validation 设置位于 [`config/`](../config/README.md)。
