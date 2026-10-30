# External

`external/` 只存放下载或检出的第三方工具、SDK 与参考源码，不放引擎内部 Rust 工具 crate，也不承载项目维护的配置。

当前主要内容：

- `slang/`：Slang 编译器、运行库、头文件和 CMake package，用于 `shader-build` 编译 shader。
- `tracy/`：Tracy profiler / capture / export 工具。
- `streamline-sdk/`：Streamline SDK 下载产物，提供头文件、链接库和 runtime DLL；该目录不进入 git。
- `rtx-remix/`：RTX Remix Runtime 参考源码，保留 `.git`，不递归下载 submodule；该目录不进入 git。
- `dlss-sdk/`：与 Remix 配套的 NGX 基础头文件、链接库和 DLSS 二进制，保留 `.git`；该目录不进入 git。
- `dlss-nr-sdk/`：Remix DLSS-NR 专用头文件和 x64/ARM64 runtime DLL，保留原包目录和许可证；该目录不进入 git。
- `rtx-gi/`：RTXGI / SHARC 参考源码 checkout，包含递归 submodule，仅作为对照实现和文档参考；该目录不进入 git。
- `rtx-di/`：RTXDI 参考源码 checkout，包含递归 submodule，仅作为对照实现和文档参考；该目录不进入 git。
- `openpbr/`：OpenPBR 参考源码 checkout，仅用于材质模型实现与文档对照；该目录不进入 git。
- `pbrtv4/`：pbrt-v4 参考源码 checkout，仅用于路径追踪、采样和材质等实现对照；许可证与第三方声明以目录内 `LICENSE.txt`、`THIRD_PARTY.md` 为准，该目录不进入 git。
- `filament/`：Filament 实时 PBR 渲染引擎 checkout，用于 BRDF/IBL、Clustered Forward、后处理和 FrameGraph 的实现对照；该目录不进入 git。
- `bevy/`：Bevy 游戏引擎 checkout，用于 ECS、Main World/Render World 提取同步、RenderGraph schedule 和 renderer 结构的实现对照；该目录不进入 git。

这些资源由根目录 `resources.toml` 描述，推荐通过 `just fetch-res` 下载或刷新。缺省情况下只处理
`download_by_default = true` 的资源；按需资源可通过名称单独下载，或使用 `just fetch-res --all` 处理全部资源。
Git resource 使用 HTTP(S)
URL，并保留 `.git` 与 submodule 元数据，方便后续确认参考源码版本。网络受限时可在运行前设置
`HTTP_PROXY` / `HTTPS_PROXY` / `NO_PROXY` 或对应小写环境变量；`fetch_res` 会继承这些环境变量，不把代理地址写入配置文件。

项目维护的 Streamline runtime JSON 与 Vulkan validation 设置位于 [`config/`](../config/README.md)。

## RTX Remix / DLSS-NR 参考资源

这组三项按需获取，不参与默认下载：

```powershell
just fetch-res rtx-remix-repo nvidia-dlss-sdk nvidia-dlss-nr-sdk
```

版本以 `resources.toml` 为准：Remix 固定到 `remix-1.6.1` 对应的
`0a6cd5b1ee814ec390c358e0c9c32ece8895ddc0`；基础 DLSS SDK 固定到该版本的 submodule commit
`374959484e79a640feaba44c93ac8cfb0a03f5b5`；NR SDK 来自同版本
[`packman-external.xml`](https://github.com/NVIDIAGameWorks/dxvk-remix/blob/remix-1.6.1/packman-external.xml)
声明的 `rtx-remix-ngx_sdk_dlnr@2`。升级时一起核对源码、基础 SDK 与 NR 扩展的接口，不单独拼接最新版。

- 源码入口：`rtx-remix/src/dxvk/rtx_render/rtx_dlss_neural_rendering.cpp` 和 `rtx_ngx_wrapper.cpp`。
- 基础 SDK：`dlss-sdk/include/` 和 `dlss-sdk/lib/Windows_x86_64/x64/`。
- NR 扩展：`dlss-nr-sdk/include/nvsdk_ngx_helpers_dlssnr_vk.h` 和
  `dlss-nr-sdk/lib/Windows_x86_64/rel/nvngx_dlssnr.dll`。

两个 SDK 保持独立目录：NR 头文件依赖基础 NGX 定义，不能仅凭 NR DLL 完成接入。7z 完整保留包内路径，
不筛选架构；使用与分发条件以各 SDK 自带的 `LICENSE.txt` 为准。
本组资源用于源码参考及后续 native 集成，不包含完整 Remix Toolkit、USD 或其它构建依赖，
不代表 DLSS-NR 已接入或已通过 GPU 验证。运行库部署应由后续 native target 使用
`truvis_register_runtime_files` 声明，不由 fetcher 复制到应用目录，也不替换现有 Streamline。

2026-10-26 调研下载的 NR 包 SHA-256 如下；这是本地验收记录，不是 NVIDIA 官方签名，fetcher 不自动校验它：

```text
4bb8651b3ac2180f23df1f39422c489810a13d445a94dc836c876c18c93ecefe
```

复用缓存或核对新下载时，对 `.temp/resource/http/<resources.toml 中完整 URL 的 SHA-256>/payload`
执行 `Get-FileHash -Algorithm SHA256`。fetcher 的相邻 `sha256` 文件只用于本地缓存一致性检查，不代替此验收记录。
旧 `.temp/resource/rtx-remix-ngx_sdk_dlnr@2.7z` 不再自动复用；新布局首次执行需要重新下载。
摘要不一致时先调查上游包变化，不直接替换验收记录。源码使用 `git -C external/rtx-remix rev-parse HEAD`
与 `git -C external/dlss-sdk rev-parse HEAD` 核对固定 commit。
