# truvis-vk-binding

对 `truvixx-vk-capi` 的 Rust 包装，当前支持 Windows x64 MSVC ABI。
直接接收 ash 类型，不生成另一套 Vulkan 对象或 descriptor DTO。

## 使用与生命周期

`Device::new(&ash::Instance, &ash::Device) -> Result<Device, LoadError>` 将设备句柄和 Instance 的
`get_device_proc_addr` 原生函数指针交给 C++。命令缺失时返回包含名称的 `LoadError`，不静默回退。

`unsafe Device::cmd_push_descriptor_set` 与 ash 对应扩展方法具有相同的命令参数：
`vk::CommandBuffer`、`vk::PipelineBindPoint`、`vk::PipelineLayout`、`u32` 和
`&[vk::WriteDescriptorSet<'_>]`，返回 `()`。

- `GfxDevice` 持有 wrapper；初始化失败时，Gfx 回收尚未交付的 Vulkan Device。
- wrapper 的私有 POD 表仅借用句柄和函数地址，不保存 ash 引用、不负责 Vulkan release。
- Device / Instance / Entry 必须保持到最后一次命令调用结束，设备子对象必须与函数表配对。
- descriptor 数组及 `pNext` 在一次同步调用内借用；不得将其生命周期扩大为静态引用。
- 命令方法的 `unsafe` 契约包括 Vulkan 有效用法、录制状态、外部同步及 GPU 资源有效期。

## 类型生成

`build.rs` 单向消费 C API header 和 native build 发布的 include roots；通过 Cargo metadata 接入现有
`just _cxx-bindings`，无需修改 CMake 对 Cargo 的依赖方向。

生成器保持 `allowlist_recursively(false)`，直接读取 `module.h`，只输出本模块函数表和导出函数。
namespace import 将 Vulkan 类型映射到 ash；可空 C 函数指针使用 `Option<ash::vk::PFN_vk…>`。
Vulkan 宿主布局直接采用 ash 的 C ABI 表示，binding 不生成单元测试。

先运行 `just cxx-debug`，再运行 `cargo build -p truvis-vk-binding`。
Release native 产物由 `just cxx` 准备，再运行 `cargo build -p truvis-vk-binding --release`。
DLL 部署及 native 入口约束见 [`truvixx-vk`](../../../modules/truvixx-vk/README.md)。
