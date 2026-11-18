# Engine Tools

`engine/tools/` 保存 workspace 的资源准备、shader 编译和 native binding 构建支持。工具通过
显式配置、文件系统输入和窄公共 API 工作，不拥有 Vulkan、RenderGraph、GPU resource 或线程生命周期。

- `truvis-fetch-res/`：读取 `resources.toml`，下载和准备模型、外部工具、SDK 与参考源码资源。
- `truvis-shader-binding-codegen/`：为 shader binding crate 提供 bindgen 参数、类型重命名、
  content hash 和 write-if-changed 支持；ABI allowlist 仍由 ABI owner 决定。
- `truvis-shader-build/`：按 `shader-packages.toml` 编译 shader，执行源码依赖、编译器 depfile、
  manifest 和输出边界检查。
- `truvis-cxx-binding-build/`：为 `cxx/bindings` 下的 Rust binding 提供 C++ header 绑定生成、
  import library 链接和显式运行文件复制。CXX native project 仍由 workspace 根的 `cxx/` 拥有。

这些工具可以依赖 `engine/runtime/e00-utils` 中的 `truvis-path` 和
`truvis-shader-manifest`；目录分类不改变 Cargo 的实际依赖方向，也不引入兼容转发层。
