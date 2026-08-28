# Shader ABI 与内存布局规则

## 1) 适用范围

- 本规则适用于所有 owner 的 `shader/abi/<owner>/`、push constants、`ConstantBuffer`、`StructuredBuffer`、`RWStructuredBuffer`、device address `PTR(...)`、vertex/input record 以及 descriptor set / binding 契约。
- 只在 shader 内部使用、不会被 Rust / C++ 写入或读取、不会进入 Vulkan pipeline layout 或 buffer layout 的局部算法结构，可按 shader 局部实现处理；一旦跨越 CPU / GPU 边界，必须按共享 ABI 处理。
- 共享 ABI 必须放在实际 owner 目录：Engine 使用 `engine/shader/abi/engine/<domain>/mod.slangi`，Renderer 使用 `renderer/shader/abi/renderer/<domain>/mod.slangi`，并分别通过 `abi/engine/mod.slangi`、`abi/renderer/mod.slangi` 暴露给对应 binding crate。Engine ABI 使用 `engine::*`，Renderer ABI 使用 `renderer::<owner>::*`；禁止在多个 owner 重复定义同一类型。

## 2) 搜索路径与依赖方向

- `.vscode/settings.json` 的 `slang.additionalSearchPaths` 固定为 `engine/shader` 与 `renderer/shader`，是编辑器看到的完整源码根；`shader-packages.toml` 是构建期唯一事实来源，必须显式镜像相同根目录或其严格子集，构建器不读取编辑器配置。
- include 必须从搜索根开始并带 layer/owner 前缀，只允许 `abi/engine/...`、`lib/engine/...`、`abi/renderer/...`、`lib/renderer/...`、`lib/sample-shader-toy/...`。禁止 `../`、绝对路径、workspace-root 路径、反斜杠以及依赖 include root 顺序解析的裸 domain 路径；同一 include 必须唯一解析。
- Shader 源码层级固定为 `abi/<owner> <- lib/<owner> <- entry`：ABI 只能依赖同 owner 或下层 owner 的 ABI；lib 可以依赖 ABI、同 owner lib 和下层 owner lib，但不能依赖 entry；entry 可以组合可见 ABI/lib，但不得被其它源码 include/import。
- Owner 方向固定为 `engine <- renderer`。Renderer shader 可以依赖 Engine；Engine shader 不得依赖 Renderer。Hello Triangle 只依赖 Engine；ShaderToy 的 package `depends_on = []`，不得依赖 Engine 或 Renderer。
- `truvis-shader-build` 必须同时执行源码预检和编译器 depfile 校验；package 只能访问自身 entry、自身 shared inputs 与 `depends_on` 传递闭包的 shared inputs，缺失声明或越界依赖必须 fail-closed。

## 3) 布局判定

- 编写或修改共享结构时，必须同时考虑 Slang、SPIR-V / Vulkan、Rust 和 C++ 的内存布局，不能只按其中任意一端的自然布局判断字段 offset、alignment 或 stride。
- 修改前必须先判定该结构使用的具体布局模型，例如 `ConstantBuffer<..., ScalarDataLayout>`、push constant、storage buffer record、buffer device address 指向的 record，或 vertex attribute input；不同模型不能混用同一套对齐假设。
- `float2` / `uint2`、`float3` / `uint3`、`float4`、`float4x4`、`uint64_t` / device address、数组元素和嵌套结构都必须显式检查 alignment 与 stride。不要假设 `float3` 自动等价于 16 字节槽，也不要假设 Rust / C++ 会替 shader 补出相同 padding。
- 不使用跨语言 ABI 不稳定的字段类型表达共享数据，例如 shader ABI 中的 `bool`、平台相关整数宽度、raw pointer 或未固定大小的枚举；需要布尔或枚举语义时，使用明确宽度的 `uint` / `int` 并在注释中说明取值。
- 矩阵字段的行列主序、坐标系语义和 CPU 写入方式属于 ABI 契约的一部分；修改字段顺序、矩阵解释或坐标约定时，必须同步检查所有 Rust / C++ 写入端和 shader 读取端。

## 4) Padding 与字段组织

- 需要 padding 时必须写成显式字段，例如 `_padding_0`，不要依赖编译器、bindgen 或宿主语言的隐式尾部填充来保证共享 ABI。
- padding 字段必须放在解决 offset 问题的位置。如果后续字段需要 8 或 16 字节对齐，而当前 offset 不满足要求，应在该字段前插入 padding；不能只在结构体末尾补齐后假定中间字段已经对齐。
- padding 注释必须说明服务的字段或边界，例如“使 `image_size` (`uint2`) 按 8 字节边界读取”或“保持结构体 stride 为 16 字节倍数”。
- 新增字段应优先填入已有明确预留 padding；如果会改变现有字段 offset 或结构 stride，必须视为 ABI breaking change，检查所有上传、绑定、读取、readback 和历史缓存使用点。
- 结构体字段顺序应优先服务 ABI 稳定和可验证布局。不要为了表面分组把标量、向量、矩阵随意穿插，导致额外 padding 或跨语言 offset 难以确认。

## 5) Rust / C++ 镜像

- Rust 侧优先使用 ABI owner 的生成 crate：Engine 使用 `truvis_shader_binding::gpu::engine::*`，全部 Renderer ABI 使用 `truvis_renderer_shader_binding::gpu::renderer::*`。已有生成绑定能表达的 push constant、scene root、material、light 或 pass-local ABI，不允许另写一份看似相同的 Rust struct。
- C++ / bindgen 路径应继续从各 binding crate 的 `ffi/rust_ffi.hpp` 和 owner Slang ABI 获得同一份结构定义，避免 Slang、C++ header 和 Rust 类型各自维护字段列表。所有 binding spec 必须声明非空 type/var allowlist；generator 固定 `allowlist_recursively(false)`。Renderer binding 必须依赖 Engine binding，并通过实际引用 namespace 显式复用 canonical Engine/base 类型，缺失映射时让 Rust 构建失败；不维护重复定义黑名单。
- 如果确实无法走现有生成绑定，必须在新增代码旁说明手写 ABI 的原因、字段 offset / size / align 的验证方式，以及它与 Slang / Vulkan 布局保持一致的依据。
- 任何 Rust `repr(C)`、C++ struct、staging buffer 写入、readback 解析或 FFI 类型，只能作为共享 ABI 的宿主侧投影；它们不能反向定义 shader ABI。

## 6) 验证要求

- 修改任何 owner 的 `shader/abi/<owner>/` 后，必须运行 `just shader`；需要绕过 package 增量缓存时运行 `just shader-force`。
- 对 push constants、嵌套结构、`uint2` / `float2` 前置 padding、`float3` 后接标量或地址字段、device address table、数组 record stride 等容易错位的改动，必须用 `spirv-dis` 检查 SPIR-V 中的 `OpMemberDecorate ... Offset`，并与生成的 Rust binding 语义一致。
- 验证时至少确认结构体 size、align、关键字段 offset、数组 stride 或 push constant range；只确认编译通过不代表 ABI 正确。
- 校验当前 shader 产物时，`GpuScene` 等 uniform buffer 使用项目启用的 standard uniform buffer layout；对应命令应包含 `spirv-val --uniform-buffer-standard-layout`，不能把缺少该设备特性参数造成的 validator 报错误判为字段迁移回归。
- 如果验证发现 Slang / SPIR-V、Rust 或 C++ 任一端 layout 不一致，应优先调整共享 Slang ABI 和显式 padding，再重新生成绑定；不要在单个调用点用硬编码 offset 临时补救。
- 修改共享 ABI 后，必须同步检查相关模块 README 或 `docs/summaries/` 是否记录了过期字段、绑定编号、buffer contract 或生命周期契约。
