# Shader ABI 与内存布局规则

## 1) 适用范围

- 本规则适用于所有 shader-visible ABI：`engine/shader/api/common/`、`engine/shader/api/pass/`、push constants、`ConstantBuffer`、`StructuredBuffer`、`RWStructuredBuffer`、device address `PTR(...)`、vertex/input record 以及 descriptor set / binding 契约。
- 只在 shader 内部使用、不会被 Rust / C++ 写入或读取、不会进入 Vulkan pipeline layout 或 buffer layout 的局部算法结构，可按 shader 局部实现处理；一旦跨越 CPU / GPU 边界，必须按共享 ABI 处理。
- 共享 ABI 的归属优先放在 `api/common/` 或 `api/pass/` 的明确职责文件中，并通过 `api/mod.slangi` 统一暴露给绑定生成流程。

## 2) 布局判定

- 编写或修改共享结构时，必须同时考虑 Slang、SPIR-V / Vulkan、Rust 和 C++ 的内存布局，不能只按其中任意一端的自然布局判断字段 offset、alignment 或 stride。
- 修改前必须先判定该结构使用的具体布局模型，例如 `ConstantBuffer<..., ScalarDataLayout>`、push constant、storage buffer record、buffer device address 指向的 record，或 vertex attribute input；不同模型不能混用同一套对齐假设。
- `float2` / `uint2`、`float3` / `uint3`、`float4`、`float4x4`、`uint64_t` / device address、数组元素和嵌套结构都必须显式检查 alignment 与 stride。不要假设 `float3` 自动等价于 16 字节槽，也不要假设 Rust / C++ 会替 shader 补出相同 padding。
- 不使用跨语言 ABI 不稳定的字段类型表达共享数据，例如 shader ABI 中的 `bool`、平台相关整数宽度、raw pointer 或未固定大小的枚举；需要布尔或枚举语义时，使用明确宽度的 `uint` / `int` 并在注释中说明取值。
- 矩阵字段的行列主序、坐标系语义和 CPU 写入方式属于 ABI 契约的一部分；修改字段顺序、矩阵解释或坐标约定时，必须同步检查所有 Rust / C++ 写入端和 shader 读取端。

## 3) Padding 与字段组织

- 需要 padding 时必须写成显式字段，例如 `_padding_0`，不要依赖编译器、bindgen 或宿主语言的隐式尾部填充来保证共享 ABI。
- padding 字段必须放在解决 offset 问题的位置。如果后续字段需要 8 或 16 字节对齐，而当前 offset 不满足要求，应在该字段前插入 padding；不能只在结构体末尾补齐后假定中间字段已经对齐。
- padding 注释必须说明服务的字段或边界，例如“使 `image_size` (`uint2`) 按 8 字节边界读取”或“保持结构体 stride 为 16 字节倍数”。
- 新增字段应优先填入已有明确预留 padding；如果会改变现有字段 offset 或结构 stride，必须视为 ABI breaking change，检查所有上传、绑定、读取、readback 和历史缓存使用点。
- 结构体字段顺序应优先服务 ABI 稳定和可验证布局。不要为了表面分组把标量、向量、矩阵随意穿插，导致额外 padding 或跨语言 offset 难以确认。

## 4) Rust / C++ 镜像

- Rust 侧优先使用 `truvis_shader_binding::gpu::*` 生成绑定，不新增手写镜像结构。已有生成绑定能表达的 push constant、scene root、material、light 或 pass-local ABI，不允许另写一份看似相同的 Rust struct。
- C++ / bindgen 路径应继续从 `engine/shader/truvis-shader-binding/ffi/rust_ffi.hpp` 和共享 Slang 头文件获得同一份结构定义，避免 Slang、C++ header 和 Rust 类型各自维护字段列表。
- 如果确实无法走现有生成绑定，必须在新增代码旁说明手写 ABI 的原因、字段 offset / size / align 的验证方式，以及它与 Slang / Vulkan 布局保持一致的依据。
- 任何 Rust `repr(C)`、C++ struct、staging buffer 写入、readback 解析或 FFI 类型，只能作为共享 ABI 的宿主侧投影；它们不能反向定义 shader ABI。

## 5) 验证要求

- 修改 `engine/shader/api/common/`、`engine/shader/api/pass/` 或任何共享 shader ABI 后，必须运行 `just shader`；需要绕过增量缓存时运行 `just shader-force`。
- 对 push constants、嵌套结构、`uint2` / `float2` 前置 padding、`float3` 后接标量或地址字段、device address table、数组 record stride 等容易错位的改动，必须用 `spirv-dis` 检查 SPIR-V 中的 `OpMemberDecorate ... Offset`，并与生成的 Rust binding 语义一致。
- 验证时至少确认结构体 size、align、关键字段 offset、数组 stride 或 push constant range；只确认编译通过不代表 ABI 正确。
- 如果验证发现 Slang / SPIR-V、Rust 或 C++ 任一端 layout 不一致，应优先调整共享 Slang ABI 和显式 padding，再重新生成绑定；不要在单个调用点用硬编码 offset 临时补救。
- 修改共享 ABI 后，必须同步检查相关模块 README 或 `docs/summaries/` 是否记录了过期字段、绑定编号、buffer contract 或生命周期契约。
