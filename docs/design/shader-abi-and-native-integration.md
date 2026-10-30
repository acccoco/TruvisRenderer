# Shader ABI 与 Native Integration 边界

> 类型：设计文档。本文只记录 owner、依赖方向和跨语言契约；字段布局细则由专项规则和 ABI 源码负责。

## 设计目标

每份 ABI 由对应 owner 维护唯一源定义：Shader ABI 属于 Slang owner，native public C ABI 属于 CXX module。生成的宿主 binding 只投影源定义，不能成为第二份字段来源。

下图箭头表示源输入到消费方；Shader 类型投影与 native C API 分别生成各自的 Rust binding。

```mermaid
flowchart LR
    subgraph Shader["Shader ABI 投影"]
        EngineAbi["Engine Slang ABI"] --> EngineHeader["Engine ffi/rust_ffi.hpp"]
        RendererAbi["Renderer Slang ABI"] --> RendererHeader["Renderer ffi/rust_ffi.hpp"]
        EngineAbi --> RendererHeader
        EngineHeader -->|bindgen| EngineBinding["truvis-shader-binding"]
        RendererHeader -->|bindgen| RendererBinding["truvis-renderer-shader-binding"]
        EngineBinding -->|类型复用| RendererBinding
    end
    subgraph Native["Native C ABI 消费"]
        CHeader["CXX module public C header"] --> NativeDll["CXX module DLL"]
        CHeader -->|bindgen| NativeBinding["cxx/rust/bindings"]
        NativeDll -->|链接产物| NativeBinding
    end
```

## Owner 与依赖

Engine ABI 位于 `engine/shader/abi/engine`，使用 `engine::*` 命名空间；Renderer ABI 位于 `renderer/shader/abi/renderer`，使用 `renderer::<owner>::*`。

Renderer 可以引用 Engine ABI，Engine 不得引用 Renderer ABI。Hello Triangle 只依赖 Engine；ShaderToy package 不依赖 Engine 或 Renderer。

Shader 源码层级固定为：

```text
abi/<owner> <- lib/<owner> <- entry
```

ABI 不依赖 entry；lib 不依赖 entry；entry 只组合自身可见的 ABI 和 lib。include 必须带 `abi/` 或 `lib/` 的 owner 前缀，禁止依赖 include root 顺序和 `../`。

依赖声明使用完整字面量路径；宏 include 和命名 module import 在预检时拒绝。预检先处理注释和续行，
再沿字面量引用检查目标文本（不受扩展名限制），并按目标 owner 自己的 package 可见范围继续检查。
条件编译的未启用分支也必须满足依赖约束。源文件相对搜索路径不得遮蔽 include root 中的目标。
编译器 depfile 只记录扁平依赖集合，用于核对实际 package 闭包；它不能替代逐边 layer 校验。
manifest 的 shared input layer 必须匹配其 include 路径中的 `abi/` 或 `lib/` 前缀，不能通过改标注放宽权限。

## Binding 责任

每个 Shader binding owner 决定：

- allowlist 中哪些 ABI 类型进入 Rust；
- 哪些类型通过 namespace re-export；
- 生成 binding 的生命周期和 crate 依赖。

Shader binding 的 `ffi/rust_ffi.hpp` 聚合 owner Slang ABI 及基础类型的 C++ 投影，供 bindgen 解析。已有生成 binding 能表达的结构不能在 Rust 中手写镜像；Renderer binding 显式复用 Engine binding 的公共类型。

Shader binding 只生成类型和常量，不生成函数、方法或构造函数。用于解析 Slang 表达式的 C++ 声明
不代表存在可链接的 native 实现；Rust 值通过字段初始化或既有的 `From` 转换构造。

## 布局不变量

跨 CPU/GPU 边界的结构必须同时检查 Slang、SPIR-V/Vulkan、Rust 和 C++ 的 size、alignment、offset 与 stride。

- 明确使用的布局模型，不能把 push constant、storage record 和 uniform buffer 的规则混用。
- padding 使用显式字段并说明服务的 offset 或 stride。
- 不使用跨语言 ABI 不稳定的 `bool`、平台相关整数宽度或未固定大小枚举。
- 矩阵主序、坐标系和 CPU 写入方式属于 ABI 契约。
- 修改 ABI 字段顺序或 stride 视为跨端变化，必须检查所有写入、绑定、读取和 readback。

具体 offset 规则见 [shader-abi-layout.md](../../.agents/rules/shader-abi-layout.md)。

Descriptor 的 Rust layout 是宿主侧投影，derive 只检查声明自身的语法与唯一 binding，不能替代 shader
资源声明和 SPIR-V 的 set/binding/type/count/stage 对照。单 binding cursor 的写入不得越过自身容量；
提交的 descriptor type、payload 数组及扩展结构数量必须一致。CPU 指针只在同步提交回调内有效，
这不改变 GPU 资源 owner 的生命周期与同步责任。

## Native 集成边界

workspace 根 `cxx/` 是唯一 native integration project。CMake target 直接依赖 native module，不镜像 Rust 顶层分层。

Native 部署集合由当前 CMake target、其运行依赖和显式 sidecar 声明决定；输出目录中的残留文件不构成部署依据。
Rust 部署前校验完整输入和删除边界，并保留失败重试所需的文件所有权；部署成功前不能把混合版本输出视为可用。

对 Rust 暴露 C ABI 的 target 必须是 DLL；内部实现可以使用 STATIC 或 OBJECT。Native public C ABI 定义在 module 的 `include/.../c_api/module.h`；`cxx/rust/bindings/` 下各 crate 的 `build.rs` 从该 header 生成 Rust binding，并链接对应 native 产物。CMake 不知道 Cargo crate，native module 不链接或回调 Rust symbol。

生成 header、CXX module 和 Rust binding 的生命周期必须由各自 owner 管理。不要把 native 句柄、CPU 指针或工作线程隐藏在通用 binding 生成器中。

## 设计不变量

- ABI 只在实际 owner 目录定义一次。
- owner 方向固定为 `engine <- renderer`。
- binding 是投影，不是 ABI 源。
- shader package 的依赖闭包和 include root 必须 fail-closed。
- ABI 改动必须验证编译产物布局，而非只验证 Rust 编译通过。

## 实现入口

- [`shader-packages.toml`](../../shader-packages.toml)
- [`renderer/shader/README.md`](../../renderer/shader/README.md)
- [`cxx/README.md`](../../cxx/README.md)
- [`truvis-shader-build`](../../engine/e00-utils/README.md)

本文不重复完整字段表；字段和 binding 编号变化应直接修改 ABI owner、生成配置和相应模块文档。

## 编译与验证边界

源码预检负责发现越界 include、package 依赖和 owner 方向问题；编译器 depfile 负责发现实际使用但未声明的 shared input。两者都不能省略。

Shader 构建器先验证整批目的地与清理边界，再修改输出。保存本轮 manifest 前先移除全部待编译任务的正式 depfile。
版本匹配但未完成的 manifest 可用于比较；每个跳过任务仍须有有效产物和通过依赖校验的正式 depfile。
编译成功、产物存在且依赖校验通过后才发布该任务 depfile；整批任务和清理全部成功后标记 manifest 完成。
这允许中断后复用已完成任务，不等于多个 SPIR-V 文件的事务回滚。

修改共享 ABI 后至少检查生成 binding、SPIR-V member offset、结构 size/align、数组 stride 或 push constant range。只通过 Rust 编译不能证明 Slang 与 Vulkan 布局一致。

CXX 目标的链接成功只能说明 native target 可链接；它不能证明 Rust caller 使用了正确的生命周期、线程或字段布局。FFI readback 还需要对照 owner 的 ABI 定义。

## 变化流程

1. 先定位 ABI owner 和使用的布局模型。
2. 检查现有 padding、生成 binding 与 C++ 投影。
3. 修改 owner ABI，并同步生成配置。
4. 运行 shader 构建和 SPIR-V 布局检查。
5. 检查 Rust/C++ 写入端、绑定端和 readback。
6. 更新模块 README 与设计边界。

## 非目标

本文不复制每个 push constant 的字段清单，不把生成 header 当作长期手写源码，也不描述某个 shader pass 的算法。

如果某个跨端结构确实不能使用生成 binding，必须在实现旁写出原因、验证方式和与 canonical ABI 的关系；不能用“当前数值相同”作为契约。

## 变更检查

- owner 是否仍然唯一？
- include 是否从固定搜索根解析？
- Renderer 是否只依赖 Engine ABI？
- 字段是否引入隐式 padding？
- Shader binding 是否仍从 owner Slang ABI 的 C++ 投影生成？
- Native Rust binding 是否仍只消费 module public C ABI，保持对 CXX 的单向依赖？
- 是否需要 `just shader` 和 `spirv-dis`？

专项规则比本文优先；本文只保留架构层面的 owner 和边界。
