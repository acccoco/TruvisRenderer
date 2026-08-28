# Engine Shader

`engine/shader/` 只保存 Engine 自有 shader 源码与 Engine Rust binding owner。workspace 级 SPIR-V 编译与
Rust binding 公共生成工具位于 `engine/e00-utils/`。所有 Renderer 侧 shader 集中在 `renderer/shader/`；package 清单的唯一来源是根目录
`shader-packages.toml`。

## 源码职责

- `abi/engine/`：Engine 的 Rust / Slang 共享 ABI。`binding`、`bindless`、`frame`、`geometry`、`light`、
  `material`、`scene` 是公共契约，`raycast` 是 Engine runtime 自有 pass ABI。
- `lib/engine/`：只在 Slang 领域复用的 Engine 通用算法，不生成 Rust binding；不得依赖 Renderer ABI、
  Renderer descriptor 或 `renderer/shader`。
- `entry/`：Engine runtime 自有 shader entry，目前只包含同步 raycast。
- `truvis-shader-binding/`：`engine::*` 与 canonical Slang 基础类型的 Rust binding owner。
- `../e00-utils/truvis-shader-binding-codegen/`：Engine/Renderer binding 共用的 bindgen 执行、类型重命名、namespace
  注入、内容 hash 与 write-if-changed 实现；header、include roots 和输出路径来自 manifest。
- `../e00-utils/truvis-shader-manifest/`：编译工具、binding codegen 与运行时路径查询共用的 manifest 模型。
- `../e00-utils/truvis-shader-build/`：读取 `shader-packages.toml` 的多 package shader 编译工具。

Renderer 侧的 ABI、Slang-only 算法、entry 与统一 Rust binding 结构见 `renderer/shader/README.md`。

## Namespace 与 binding owner

- Engine ABI：`engine::*`，Rust owner 为 `truvis-shader-binding`。
- Renderer ABI：`renderer::render_passes::*` 与 `renderer::kit::*`，Rust owner 为
  `truvis-renderer-shader-binding`。

公共 generator 在内部固定 `allowlist_recursively(false)`，并拒绝没有 type/var allowlist 的 spec。
Engine binding 显式 allowlist canonical 基础类型与 `engine::*`；Renderer binding 只 allowlist
`renderer::*`，并通过 `ModuleRawLine` 显式复用 Engine canonical Rust 类型。缺少跨 owner re-export
时应由 Rust 编译直接失败，不维护容易过期的禁止定义黑名单。

Engine binding、shader 源码和 package 配置禁止依赖任何 Renderer crate 或 `renderer/shader`。

## Package、include 与产物

当前只保留四个 package：

| package | 依赖 | 相对 `shader_build` 的产物前缀 |
| --- | --- | --- |
| `engine` | 无 | `engine/` |
| `renderer` | `engine` | `renderer/` |
| `sample-hello-triangle` | `engine` | `samples/hello-triangle/` |
| `sample-shader-toy` | 无 | `samples/shader-toy/` |

四个 package 的 include roots 只使用 `.vscode/settings.json` 声明的 `engine/shader`、`renderer/shader` 或其严格
子集，不使用 workspace 根目录 `.`。当前 manifest 因此要求源码 include 写成 `abi/engine/...`、
`lib/engine/...`、`abi/renderer/...`、`lib/renderer/...` 或 `lib/sample-shader-toy/...`。构建器本身不识别
固定 owner 名称，而是从 `shared_inputs` 相对 include root 的 `layer/owner` 前缀推导合法路径。

执行：

```text
just shader
just shader-force
```

`just shader` 先运行 package-aware 编译器，再构建 Engine 与 Renderer 两个 binding crate。运行时通过
`ShaderArtifactPath::resolve(package_id, entry_relative_path)` 解析同一份 manifest，调用点不手写输出前缀。
`paths.toml` 的 `dirs.shader_build`、`dirs.binding_build` 和 `dirs.temp` 是产物物理根的唯一配置源；
`shader-packages.toml` 只声明 package/compiler/binding 及 package id 到输出前缀的逻辑映射。

`build/shader/.state/shader-build.json` 使用版本 6 记录 package、compiler、输出、共享输入和任务，
`build/shader/.deps/` 保存编译器 depfile。构建前检查 canonical include、唯一解析和层级方向，编译后再验证
真实传递依赖。单个 entry 变化只重编该
任务；Renderer shared input 只失效 Renderer；Engine shared input 会传播到声明依赖 Engine 的 Renderer 和 Hello Triangle；
ShaderToy shared input 只失效自身。

binding 源码生成到 `dirs.binding_build/{TARGET}/shader/<binding-crate>/`（当前为
`build/bindings/{TARGET}/shader/<binding-crate>/`），源码树不保存
`_shader_bindings.rs`。两个 owner 的 `build.rs` 按 binding id 读取 `[[binding]]`，只补充各自 allowlist 与
跨 crate Rust re-export policy。

## ABI 布局约束

`abi/` 同时服务 Slang/SPIR-V、C++ bindgen 和 Rust `repr(C)`。字段顺序、padding、size、align、
descriptor set/binding 与 push constant range 都是契约；namespace 或文件移动不得改变它们。

新增或调整 ABI 后执行 `just shader-force`，并使用
`spirv-val --uniform-buffer-standard-layout` 与 `spirv-dis` 核对关键结构；Rust binding 编译同时执行
bindgen 生成的 size/align/offset 静态断言。更完整规则见 `.agents/rules/shader-abi-layout.md`。

## 后续演进

当前 `lib/` 仍以 include 为主。稳定算法可在独立阶段逐步改成 Slang module / `.slang-module`，但这只
优化前端编译；最终每个 SPIR-V 仍是自包含程序，不引入运行时 shader 动态链接。
