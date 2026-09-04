# truvixx-vk

使用 Vulkan C headers 的按需命令桥接。唯一 public target 为 `truvixx-vk-capi` SHARED library，
当前提供设备函数表初始化和 `vkCmdPushDescriptorSetKHR` 转发。

## 入口与所有权

- 调用方提供 `VkDevice` 和同一 ash Instance 派生的原生 `PFN_vkGetDeviceProcAddr`。
- `truvixx_vk_device_init` 查询一次目标命令，成功填充 `TruvixxVkDeviceDispatch`；失败返回 `VK_FALSE`，
  非空输出结构保持清零。初始化成功后，命令接口同步调用缓存的函数指针。
- 函数表由 Rust 按值持有，不分配 native heap，不创建或销毁 Vulkan 对象，也不持有 DLL handle。
- native target 仅链接 `Vulkan::Headers`，定义 `VK_NO_PROTOTYPES`，不使用 Vulkan-Hpp 或系统 loader 导入库。
- 传入的入口已经属于 ash 选择的系统 loader / Streamline 链路，不需要调用 Rust trampoline。

## 同步与 ABI

所有调用沿 Rust RenderThread 同步完成。descriptor 数组、image/buffer info、加速结构 `pNext` 和其他嵌套指针
只在调用期间借用，native 不复制或保存它们；引用的 GPU 资源仍须保持到相关提交完成。

`module.h` 是 public C ABI，Rust binding 直接复用 ash 的 Vulkan C ABI 类型。
两端必须使用同一目标平台的 Vulkan 宿主布局；native 保留自定义函数表的 POD 与 Windows x64 大小、对齐检查。
标准 Vulkan 类型的布局由 ash 和 Vulkan C headers 提供，本模块不维护额外的逐字段校验。

## 构建

使用 workspace 的 `just cxx-debug` 或 `just cxx`。CMake 在自身 binary dir 生成 Vulkan include roots，
`truvixx-vk-bindgen-inputs` 在 native build 成功后发布到
`build/cxx/generated/include/TruvixxVk/c_api/vulkan_include_roots.txt`。
单独 configure（包括 clangd 辅助 configure）不发布该文件，Rust 因而消费实际构建使用的 headers。

Rust consumer 和函数签名见 [`truvis-vk-binding`](../../rust/bindings/truvis-vk-binding/README.md)。
