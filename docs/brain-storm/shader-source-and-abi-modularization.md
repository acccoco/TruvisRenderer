# Shader 源码与 ABI 模块化方案

> 状态：源码集中化、四 package、统一 App binding 与 strict allowlist 已于生产 workspace 实施；
> 阶段 5 Slang module 化仍是可选优化。
> 当前实现事实以 `engine/shader/README.md`、App 模块 README、`docs/summaries/` 和源码为准；本文保留方案推导、
> 迁移记录与实验依据。

## 目标与结论

Shader 代码按实际共享边界拆成三个互不混淆的职责：

- `lib/`：纯 Slang 算法实现，只在 Slang 领域通过 `include` / `import` 复用，不生成 Rust binding。
- `entry/`：最终 shader entry point，只参与目标 SPIR-V 生成，不被其它 Slang 代码复用，也不生成 Rust binding。
- `abi/`：Rust / Slang 共享的 shader-visible ABI，承载结构、常量、descriptor set / binding、push constant、
  buffer record 等跨 CPU/GPU 契约，并按 owner 生成 Rust binding。

目录名采用 `abi/`，不采用 `share/`：`abi` 能直接表达内存布局和绑定契约，也不会和同样具有“共享”含义的
Slang `lib/` 混淆。

本方案的直接结论是：

- App shader entry 可以引用 Engine `lib/` 和 Engine `abi/`。
- Engine 与 App 分别拥有自己的 ABI；全部 App ABI 共用一个 binding crate。
- Cargo 依赖只能是 App binding 依赖 Engine binding，Engine 不反向依赖 App。
- 每个 ABI 类型只有一个生成 owner；namespace 负责名称边界，strict allowlist 与 canonical re-export
  负责唯一 Rust 类型所有权。
- Slang precompiled module 可作为后续编译性能优化，但不是第一阶段目录拆分的前置条件。

## 实施前基线

实施前所有 shader 源码位于 `engine/shader/`：

- `entry/` 保存 Engine runtime、App pass 与 sample 的所有入口。
- `api/common/` 保存 Engine 公共 ABI。
- `api/pass/` 同时混有 Engine runtime、App render pass 和 App GUI 的 pass-local ABI。
- `lib/` 保存 Slang 算法实现。
- `truvis-shader-binding` 通过单一 `api/mod.slangi` 聚合入口生成全部 Rust binding。
- `shader-build` 只扫描一个 `engine/shader/entry`，只配置一个 include root，并把 `api/`、`lib/` 和
  entry include 文件视为全局 shared inputs。

这套旧实现能工作，但物理所有权和项目的 App → Engine 依赖方向不完全一致；任意 App ABI 变化也会刷新统一 binding，
任意共享输入变化会保守重编所有 entry。

## 目标目录与 owner

```text
engine/shader/
├─ abi/
│  └─ engine/                        # Engine 公共 ABI
│     ├─ mod.slangi
│     ├─ binding/
│     ├─ bindless/
│     ├─ frame/
│     ├─ geometry/
│     ├─ light/
│     ├─ material/
│     ├─ scene/
│     └─ raycast/                    # Engine runtime 自有 pass ABI
├─ lib/
│  └─ engine/                        # 纯 Slang 算法
│     ├─ core/
│     ├─ lighting/
│     └─ scene/
├─ entry/
│  └─ raycast/                       # Engine runtime 自有 entry
├─ truvis-shader-binding/            # Engine ABI canonical Rust 类型
├─ truvis-shader-binding-codegen/    # 多 binding crate 复用的 bindgen helper
└─ truvis-shader-build/              # 多 package SPIR-V 构建工具

app/shader/
├─ abi/
│  └─ app/                           # 全部 App CPU/GPU ABI
│     ├─ mod.slangi
│     ├─ render_passes/
│     └─ kit/
├─ lib/
│  ├─ app/                           # App 的 Slang-only 算法
│  └─ sample-shader-toy/             # ShaderToy 独立 include
├─ entry/
│  ├─ app/                           # render pass + ImGui
│  ├─ sample-hello-triangle/
│  └─ sample-shader-toy/
└─ truvis-app-shader-binding/        # 全部 App ABI 的 Rust binding
```

现有内容按 owner 迁移：

| 当前内容 | 目标 owner |
| --- | --- |
| frame、scene、geometry、material、light、bindless、全局 binding | `engine/shader/abi/engine` |
| raycast ABI 与 entry | Engine render runtime / `engine/shader` |
| realtime/offline RT、raster、post、resolve、sdr、selection、image clear | `app/shader/abi/app/render_passes` + `entry/app` |
| ImGui ABI 与 entry | `app/shader/abi/app/kit` + `entry/app/ui` |
| hello-triangle、shader-toy entry | `app/shader/entry/<sample>` |
| PBR、sampling、scene access 等不依赖 App resource 的通用算法 | `engine/shader/lib/engine` |
| realtime/offline RT、ReSTIR、SHARC、GBuffer、pass env map 等依赖 App ABI/resource 的算法 | `app/shader/lib/app` |

## 依赖与组合规则

### Slang 侧

```text
App entry  ──#include──> App ABI
App entry  ──#include──> Engine ABI（可由 App ABI 间接引入）
App entry  ──#include/import──> App-local lib / Engine lib
App-local lib ──#include──> App ABI / Engine ABI
Engine lib ──import/include──> 其它 Engine lib
```

- `abi/` 必须保持 Slang 与 C++/Clang 都能解析的公共语法子集，使用 `#include + #pragma once`。
- `lib/` 是纯 Slang，可逐步使用 `module`、`import`、`public/internal` 和可选 `.slang-module`。
- `entry/` 只声明 entry point 和组合调用，不作为其它源码的 include/import 目标。
- include 从 `.vscode/settings.json` 声明的 `engine/shader`、`app/shader` 搜索根开始，必须使用
  `abi/engine`、`lib/engine`、`abi/app`、`lib/app` 或 `lib/sample-shader-toy` 前缀并唯一解析。
- 层级方向为 `abi/<owner> <- lib/<owner> <- entry`，owner 方向为 `engine <- app`；同 owner lib 可以互相
  复用，但 ABI 不得依赖 lib，任何 lib 都不得依赖 entry。
- 引用 App pass resource、push constant 或 App-only record 的算法必须归 App-local `lib/`；Engine `lib/` 不得引用 App shader root。
- 不把依赖 `PTR`、`__SLANG__`、descriptor 声明等预处理语义的 ABI 机械迁移为 Slang module；`import`
  不传播预处理宏状态。

最终每个 SPIR-V 仍是自包含程序。Engine lib 代码进入所有使用它的目标 SPIR-V 是正常的静态链接结果，
不属于符号重复定义。

### Cargo 侧

```text
truvis-app-shader-binding ──> truvis-shader-binding
App runtime               ──> truvis-app-shader-binding
truvis-shader-binding     -X-> 任何 App crate
```

Engine binding crate 唯一定义基础向量类型和 Engine ABI。App binding crate 的 Clang 输入可以 include Engine ABI，
但生成结果不得重新定义 Engine 类型。

## Namespace 与 Rust 路径

共享 ABI 使用 owner namespace，不使用无归属的顶层 `frame`、`scene`、`realtime_rt`：

```slang
namespace engine
{
namespace frame
{
struct PerFrameData { /* ... */ };
}
}
```

```slang
namespace app
{
namespace render_passes
{
namespace realtime_rt
{
struct PushConstants { /* ... */ };
}
}
}
```

目标 Rust 路径分别为：

```rust
truvis_shader_binding::gpu::engine::frame::PerFrameData
truvis_app_shader_binding::gpu::app::render_passes::realtime_rt::PushConstants
```

namespace 变化只改变源码和 Rust symbol path，本身不应改变 size、align、offset、descriptor 编号或 GPU 资源语义；
迁移验证仍必须按 shader ABI 规则检查这些契约。

## Binding 生成与唯一类型所有权

### Engine binding

Engine wrapper header include Slang 基础类型 shim、`abi/engine/mod.slangi` 与 Engine 自有
`abi/engine/raycast/mod.slangi`。Engine binding 生成：

- `Float2/3/4`、`Float4x4`、`Uint2/3/4` 等 canonical 基础类型；
- `engine::*` 下的全部 Engine ABI；
- Engine 自有 descriptor/binding 常量。

现有 `truvis-shader-binding` crate 名称可以保留，避免无关 crate rename。

### App binding

App wrapper header 同时 include Engine ABI 和 App 自有 ABI，让 Clang 完成完整语义解析。bindgen 必须采用：

1. 只 allowlist `app::*`。
2. `allowlist_recursively(false)`，阻止递归生成基础类型和 Engine ABI。
3. 在 bindgen 生成结果的实际引用 module 中注入 Engine canonical type re-export。
4. 缺失 canonical re-export 时让 Rust 编译失败，以 fail-closed 暴露 owner 或依赖错误。

当前生产 App ABI 只按值使用 canonical 基础向量/整数类型，生成器在 `root` 注入这些类型即可。若以后 App ABI
按值嵌入 Engine namespace struct，只在生成文件顶层写 `pub use ...::engine` 不够：bindgen 会为 C++ namespace
生成空的 `root::engine::*` module并遮蔽顶层 re-export。此时必须通过公共 helper 的 `ModuleRawLine`
把 canonical type 注入实际引用位置，例如：

```rust
// bindgen module: root
pub use truvis_shader_binding::gpu::{Float4, Uint2, uint};

// bindgen module: root::engine::frame
pub use truvis_shader_binding::gpu::engine::frame::PerFrameData;
```

App ABI 按值嵌入 Engine ABI 时，生成字段必须仍指向 Engine crate 的同一个 Rust 类型。类型相同由 Rust 编译器证明，
不靠 size 相同或手写转换猜测；未配置对应 module re-export 时应视为 codegen 配置错误。

### 公共 codegen helper

新增单一 `truvis-shader-binding-codegen` build-support crate，收敛：

- bindgen/Clang 参数；
- Slang 基础类型命名回调；
- 非空 allowlist 校验、固定 `allowlist_recursively(false)` 与 namespace module raw-line 注入；
- 生成路径、内容 hash 和 write-if-changed；
- `rerun-if-changed` 输入声明。

Engine/App binding 的 `build.rs` 只声明各自 header、include roots、owner namespace 和 external type mapping，
不复制生成实现。

## 常量与 descriptor 编号

现有 `GLOBAL_SETS_COUNT`、`*_SET_NUM` 使用宏，bindgen 会把宏放到生成 crate 的 root，无法体现 owner namespace。
目标设计优先使用 namespaced `static const uint`：

```slang
namespace engine { namespace binding {
static const uint GLOBAL_SETS_COUNT = 3;
}}

namespace app { namespace render_passes { namespace realtime_rt {
static const uint SET_NUM = engine::binding::GLOBAL_SETS_COUNT;
}}}
```

可行性实验已证明该常量可以用于 `[[vk::binding(0, SET_NUM)]]`，bindgen 也能生成 App namespace 下的
`SET_NUM`。迁移时仍需逐项确认所有 descriptor set 顺序保持不变。

## 多 package Shader Build

`shader-build` 从单根目录模型改成显式 package 模型。workspace 根部的
`shader-packages.toml` 作为唯一 package 清单，每个 package 至少声明：

```text
id
entry_root
include_roots[]
shared_input_roots[]
depends_on[]
output_prefix
```

示例关系：

```text
engine                 -> build/shader/engine/...
app                    -> build/shader/app/...
sample-hello-triangle  -> build/shader/samples/hello-triangle/...
sample-shader-toy      -> build/shader/samples/shader-toy/...
```

构建不变量：

- `package id + entry relative path` 唯一标识编译任务。
- 所有输出带 package prefix，禁止不同 App 的同名 entry 覆盖。
- package 依赖必须无环；App package 可以依赖 Engine，Engine package 不依赖 App。
- `depends_on` 同时约束增量传播与真实源码依赖；Slang、GLSLC、DXC 的 depfile 必须在写入增量状态前通过
  校验，只允许当前 entry、本 package shared inputs 和传递依赖 package 的 shared inputs。
- Engine ABI/lib 变化使所有依赖 package 失效；App local ABI/lib/include 只使本 package 失效。
- ShaderToy 使用独立 include roots 和 `depends_on = []`，任何 Engine/App 变化都不得使它失效。
- manifest 记录 package 配置、编译器参数版本、输入 stamp 和输出路径；package 配置变化必须使相关任务失效。
- stale output 仍只按旧 manifest 声明的管理范围删除；旧 depfile 和输出根内空目录由构建器安全清理。

运行时 shader 路径同步增加 package 维度，避免调用点手写产物前缀。具体 API 可以继续归属 `TruvisPath`，
但必须由 package id 和相对 entry path 推导唯一 SPIR-V 路径。

## 迁移阶段

各阶段保持一个可验证行为，不把机械移动、namespace rename、生成器变化和 Slang module 化混在一个提交中。

### 阶段 1：抽取 binding codegen helper（已完成）

- 从现有 Engine binding `build.rs` 提取公共生成逻辑。
- 保持当前单一 ABI 聚合入口和生成 Rust API 不变。
- 证明 `just shader-force`、Engine binding 和现有调用方无行为变化。

### 阶段 2：建立 Engine canonical namespace（已完成）

- 把现有 common ABI 移入 `engine::*`。
- `raycast` 作为 Engine runtime ABI 一并归位。
- 更新 Slang 与 Rust 调用路径，不移动 App ABI/entry。
- 用 SPIR-V offset、Rust size/align 和现有运行路径证明 ABI 未变。

### 阶段 3：拆出并统一 App binding crate（已完成）

- 添加 `truvis-app-shader-binding`，聚合 `app::render_passes::*` 与 `app::kit::*`。
- 使用精确 module re-export 复用 Engine canonical 类型。
- strict allowlist 固定禁止递归生成其它 owner；缺失映射时构建直接失败。

### 阶段 4：多 package 编译与 entry 移动（已完成）

- 引入 package manifest 和 namespaced output，将全部 App 源码集中到 `app/shader`。
- 收敛为 Engine、App、Hello Triangle、ShaderToy 四个 package。
- 同步更新所有 pipeline shader artifact path。
- 验证 Engine runtime、主体 App 和 samples 都加载各自 SPIR-V。

### 阶段 5：可选 Slang module 化（未实施）

- 为稳定的 Engine `lib/` 建立 primary module 与访问控制。
- entry 从文本 include 逐步改为 `import`。
- 如编译时间值得优化，再生成 `.slang-module` 并加入版本/参数/源码失效检查。
- 不在本阶段改变 shader 算法、specialization 语义或运行时 pipeline 组合。

每个阶段同步更新最接近的 README / summary；实现完全落地后，本文只保留仍未完成的演进方向，当前事实提炼到
`engine/shader/README.md`、App 模块 README 和 layering summary。

## 必须保持的设计约束

- Engine 不依赖任何 App crate、App shader root 或 App binding。
- 每个 shader-visible ABI 类型只有一个 Slang 定义和一个 Rust binding owner。
- App binding 中 re-export 的 Engine 类型必须与 Engine binding 类型同一，而不是布局相同的副本。
- `lib/` 中纯算法结构不进入 Rust binding；一旦被 CPU 读写或进入 pipeline layout，就必须迁入 owner `abi/`。
- namespace、文件移动和 crate 拆分不得改变字段顺序、padding、size、align、descriptor set/binding、push constant range。
- 最终 SPIR-V 自包含；`.slang-module` 只优化前端编译与链接，不引入运行时动态链接。
- shader build manifest 和 Cargo binding 生成必须能由源码与配置完全重建，不依赖手工复制的生成文件。
- include 路径和 compiler depfile 必须共同证明 package 边界，不能只依赖 Review 约定。

## 生产实施与验证结果

阶段 1-4 已在生产 workspace 落地。实施中进一步确认：realtime/offline RT payload、pass env map 等源码
直接引用 App pass resource / ABI，因此按实际 owner 放入
`app/shader/lib/app/`；Engine `lib/engine/` 只保留不依赖 App 的通用算法。

验证结果：

- `just shader-force`：4 个 package、29 个 entry 全量编译通过，分布为 Engine 4、App 22、
  Hello Triangle 1、ShaderToy 2。
- 再次 `just shader`：`compiled=0, skipped=29`。
- App shared input 探针：`compiled=22, skipped=7`。
- Engine shared input 探针：重编 Engine 与所有依赖 package，`compiled=27, skipped=2`；独立 shader-toy 未失效。
- ShaderToy lib 探针为 `compiled=2, skipped=27`；Hello Triangle entry 探针为
  `compiled=1, skipped=28`。
- Engine/App 两套 binding crate 均通过构建；统一 App 生成文件包含 10 个 Rust-visible App struct，
  不生成 Engine struct。另有 2 个 SHARC buffer record 由 `#ifdef __SLANG__` 明确保持 shader-only。
- 临时移除 canonical `Float4x4` re-export 后，App binding 按预期因类型缺失构建失败；恢复后通过。
  Cargo tree 确认 `truvis-app-shader-binding -> truvis-shader-binding`，反向依赖不存在。
- 29 个 SPIR-V 全部通过
  `spirv-val --target-env vulkan1.3 --uniform-buffer-standard-layout`；`spirv-dis` 确认 raycast、realtime RT、
  image clear 的关键 offset 与 descriptor set/binding 保持原契约。
- `cargo check --workspace`、`cargo fmt --all -- --check`、`git diff --check` 与
  `cargo build --bin truvis-app` 通过。
- 使用 Vulkan validation 环境启动 `build/debug/truvis-app.exe`，实际执行 RT、HDR→SDR、resolve、
  coordinate-gizmo 与 GUI RenderGraph，并完成 present。通过 `Truvis Editor` 窗口正常关闭后，
  RenderThread、Streamline、Vulkan device/instance 与 winit event loop 完整释放，进程以 0 退出。

## 可行性实验

实验源码位于本地可丢弃目录 `.temp/shader-modularization-poc/`，未加入生产 workspace。实验使用当前项目环境：

- Slang `2026.11`
- bindgen `0.72.1`
- Vulkan SDK `spirv-val` / `spirv-dis`
- Rust 2024 edition

### 实验覆盖

- Engine `lib` 定义 `module algorithms`，App entry 通过 `import algorithms` 调用。
- Engine/App ABI 分别使用 `engine` 与 `app::render_passes` namespace。
- App namespaced `SET_NUM` 引用 Engine namespaced `GLOBAL_SETS_COUNT`，并用于 `vk::binding`。
- App `PushConstants` 按值嵌入 Engine `PerFrameData`，同时包含 Engine canonical `Float4`。
- Engine/App 分别生成 binding crate，App crate 正向依赖 Engine crate。
- App bindgen 解析 Engine ABI，但不生成 Engine/base struct，通过精确 module re-export 引用 canonical 类型。
- consumer 同时构造 Engine 类型和 App push constant，编译器验证字段类型身份，运行时验证 size 为 32 字节。
- Slang 分别从源码 module 和预编译 `.slang-module` 生成 SPIR-V。

### 首轮失败及结论

首轮只在生成文件 `root` 注入：

```rust
pub use engine_binding::gpu::engine;
```

Cargo 编译失败。bindgen 为字段引用生成了空的 `root::engine::frame` module，遮蔽顶层 re-export；
同时 App 常量需要 `root::uint`，但基础 alias 未注入。

修正为在 `root` 注入基础类型，并在 `root::engine::frame` 注入 `PerFrameData` 后，实验通过。
因此“只 allowlist App namespace”还不够，生产 codegen 必须拥有精确 external module/type mapping。

### 实测结果

Rust：

- `cargo check --workspace`：通过。
- `cargo run -p consumer`：通过，`SET_NUM == 3`，`PushConstants` size 为 32。
- `cargo tree -p app-binding`：确认 `app-binding -> engine-binding`。
- 生成的 App binding 定义 `PushConstants`，字段类型为 canonical Engine `PerFrameData` / `Float4`。
- App binding 未定义 `Float2/3/4`、`Float4x4`、`Uint2/3/4` 或 `PerFrameData` struct。

Slang/SPIR-V：

- 源码 `import` 编译：通过。
- `algorithms.slang-module` 预编译及仅通过 module 搜索路径链接：通过。
- 两条路径产物均通过 `spirv-val --target-env vulkan1.3`。
- 两个 SPIR-V 均为 1204 字节，SHA-256 完全相同：
  `5B9DAEF909D1EC97307E31353BC54496D900C93A4621AFE7382E88F99F4B68F6`。
- 反汇编确认 compute entry 为 `main`，App output 为 `DescriptorSet 3 / Binding 0`。
- `PerFrameData.tint` offset 为 0；`PushConstants.frame_data` / `color` offset 分别为 0 / 16。

### 实验边界

已证明的是最小机制闭环，不代表完整迁移已经完成：

- 尚未移动任何生产 shader、ABI 或 Rust 调用点。
- 尚未验证主体 App、Cornell、GUI、raycast 与所有 RT shader 的全量构建/运行。
- 尚未实现 package manifest、per-package 增量失效和 namespaced runtime artifact path。
- 尚未覆盖数组、复杂嵌套、device address、全部 descriptor set 或 ray tracing shader group。
- `.slang-module` 的版本/参数/source freshness 管理尚未接入当前 manifest。

## 后续决策

`abi/`、binding owner 和 package 边界已经固定。后续只剩阶段 5 是否值得实施：先采集当前 Slang 全量/增量编译耗时，
只有 Engine 稳定 `lib/` 的前端重复编译成为明确瓶颈时，再引入 module / `.slang-module`。该阶段必须独立处理
Slang 版本、编译参数、source freshness 与 cache invalidation，不与 shader 算法或 pipeline 语义修改混合。
