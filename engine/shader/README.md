# Engine Shader

`engine/shader/` 只保存 Engine 自有 shader 源码，并提供 workspace 级 SPIR-V 编译与 Rust binding
生成基础设施。所有 App 侧 shader 集中在 `app/shader/`；package 清单的唯一来源是根目录
`shader-packages.toml`。

## 源码职责

- `abi/engine/`：Engine 的 Rust / Slang 共享 ABI。`binding`、`bindless`、`frame`、`geometry`、`light`、
  `material`、`scene` 是公共契约，`raycast` 是 Engine runtime 自有 pass ABI。
- `lib/engine/`：只在 Slang 领域复用的 Engine 通用算法，不生成 Rust binding；不得依赖 App ABI、
  App descriptor 或 `app/shader`。
- `entry/`：Engine runtime 自有 shader entry，目前只包含同步 raycast。
- `truvis-shader-binding/`：`engine::*` 与 canonical Slang 基础类型的 Rust binding owner。
- `truvis-shader-binding-codegen/`：Engine/App binding 共用的严格 allowlist、类型重命名、namespace
  注入、生成路径、内容 hash 与 write-if-changed 实现。
- `truvis-shader-build/`：读取 `shader-packages.toml` 的多 package shader 编译工具。

App 侧的 ABI、Slang-only 算法、entry 与统一 Rust binding 结构见 `app/shader/README.md`。

## Namespace 与 binding owner

- Engine ABI：`engine::*`，Rust owner 为 `truvis-shader-binding`。
- App ABI：`app::render_passes::*` 与 `app::kit::*`，Rust owner 为
  `truvis-app-shader-binding`。

公共 generator 在内部固定 `allowlist_recursively(false)`，并拒绝没有 type/var allowlist 的 spec。
Engine binding 显式 allowlist canonical 基础类型与 `engine::*`；App binding 只 allowlist
`app::*`，并通过 `ModuleRawLine` 显式复用 Engine canonical Rust 类型。缺少跨 owner re-export
时应由 Rust 编译直接失败，不维护容易过期的禁止定义黑名单。

Engine binding、shader 源码和 package 配置禁止依赖任何 App crate 或 `app/shader`。

## Package、include 与产物

当前只保留四个 package：

| package | 依赖 | 产物前缀 |
| --- | --- | --- |
| `engine` | 无 | `build/shader/engine/` |
| `app` | `engine` | `build/shader/app/` |
| `sample-hello-triangle` | `engine` | `build/shader/samples/hello-triangle/` |
| `sample-shader-toy` | 无 | `build/shader/samples/shader-toy/` |

四个 package 的 include roots 只使用 `.vscode/settings.json` 声明的 `engine/shader`、`app/shader` 或其严格
子集，不使用 workspace 根目录 `.`。源码 include 必须写成 `abi/engine/...`、`lib/engine/...`、
`abi/app/...`、`lib/app/...` 或 `lib/sample-shader-toy/...`，不得依赖搜索顺序。

执行：

```text
just shader
just shader-force
```

`just shader` 先运行 package-aware 编译器，再构建 Engine 与 App 两个 binding crate。运行时通过
`TruvisPath::shader_build_path_str(package_id, entry_relative_path)` 解析同一份 manifest，调用点不手写
输出前缀。

`build/shader/.state/shader-build.json` 记录 package 配置、共享输入和任务，`build/shader/.deps/` 保存编译器
depfile。构建前检查 canonical include 与层级方向，编译后再验证真实传递依赖。单个 entry 变化只重编该
任务；App shared input 只失效 App；Engine shared input 会传播到声明依赖 Engine 的 App 和 Hello Triangle；
ShaderToy shared input 只失效自身。

binding 源码生成到 `build/bindings/{TARGET}/shader/<binding-crate>/`，源码树不保存
`_shader_bindings.rs`。

## ABI 布局约束

`abi/` 同时服务 Slang/SPIR-V、C++ bindgen 和 Rust `repr(C)`。字段顺序、padding、size、align、
descriptor set/binding 与 push constant range 都是契约；namespace 或文件移动不得改变它们。

新增或调整 ABI 后执行 `just shader-force`，并使用
`spirv-val --uniform-buffer-standard-layout` 与 `spirv-dis` 核对关键结构；Rust binding 编译同时执行
bindgen 生成的 size/align/offset 静态断言。更完整规则见 `.agents/rules/shader-abi-layout.md`。

## 后续演进

当前 `lib/` 仍以 include 为主。稳定算法可在独立阶段逐步改成 Slang module / `.slang-module`，但这只
优化前端编译；最终每个 SPIR-V 仍是自包含程序，不引入运行时 shader 动态链接。
