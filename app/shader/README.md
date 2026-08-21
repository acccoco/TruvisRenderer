# App Shader

`app/shader/` 是所有 App 侧 shader 源码和 App Rust binding 的唯一物理根目录。Rust crate 仍按
`app-render-passes`、`app-kit` 和 sample 分工，但 shader package 按相同依赖、include roots、增量失效
范围和发布边界组织，不跟随 Rust crate 目录拆散。

## 目录职责

```text
app/shader/
├─ abi/
│  └─ app/
│     ├─ mod.slangi
│     ├─ render_passes/
│     └─ kit/
├─ lib/
│  ├─ app/
│  └─ sample-shader-toy/
├─ entry/
│  ├─ app/
│  ├─ sample-hello-triangle/
│  └─ sample-shader-toy/
└─ truvis-app-shader-binding/
```

- `abi/app/` 是 Rust / Slang 共享契约，保持 `app::render_passes::*` 与
  `app::kit::*` namespace；统一聚合入口是 `abi/app/mod.slangi`。
- `lib/app/` 保存依赖 App ABI/resource 的 RT、ReSTIR、SHARC、GBuffer、raster 与环境贴图
  算法，只在 Slang 侧共享，不生成 Rust binding。
- `lib/sample-shader-toy/` 只对 ShaderToy 可见，不得引用 Engine 或 App。
- `entry/` 只保存最终编译入口，不作为其它 shader 的共享接口，也不生成 Rust binding。
- `truvis-app-shader-binding/` 是全部 App ABI 的唯一 Rust binding owner，正向依赖
  `truvis-shader-binding`。

## 四个 package

| package | entry root | 可见 include roots | 依赖 |
| --- | --- | --- | --- |
| `engine` | `engine/shader/entry` | `engine/shader` | 无 |
| `app` | `entry/app` | `engine/shader`、`app/shader` | `engine` |
| `sample-hello-triangle` | `entry/sample-hello-triangle` | Engine | `engine` |
| `sample-shader-toy` | `entry/sample-shader-toy` | `lib/sample-shader-toy` | 无 |

manifest 不提供 workspace 根目录 `.` include root。搜索根与 `.vscode/settings.json` 保持一致，构建器不读取
编辑器配置。include 必须从搜索根开始并带 owner：App 使用 `abi/app/...`、`lib/app/...`，Engine 使用
`abi/engine/...`、`lib/engine/...`，ShaderToy 使用 `lib/sample-shader-toy/...`。每个路径必须唯一解析，
禁止 `../`、绝对路径、反斜杠和依赖搜索顺序的裸 domain 路径。

依赖层级固定为 `abi/<owner> <- lib/<owner> <- entry`；ABI 不能依赖 lib/entry，lib 不能依赖 entry。
Owner 方向固定为 `engine <- app`。构建器先静态检查源码，再用 Slang/GLSLC/DXC depfile 验证实际传递依赖；
ShaderToy 即使物理上位于 `app/shader`，也不能访问 App 或 Engine shared inputs。

## Binding 唯一所有权

`truvis-app-shader-binding` 只 allowlist `app::*`，generator 固定
`allowlist_recursively(false)`。canonical Slang 基础类型从 `truvis-shader-binding` 显式 re-export；未来
App ABI 若引用具名 Engine 类型，必须通过 `ModuleRawLine` 注入到 bindgen 实际生成的 Engine namespace。
缺失映射时允许 Rust 编译失败，以 fail-closed 暴露依赖错误。

Engine 类型只由 `truvis-shader-binding` 定义；不使用禁止定义黑名单，也不为旧 App binding crate 或旧
shader 路径保留兼容层。

## 增量边界

- `app/shader/abi/app` 或 `lib/app` 变化只重编 App 的 22 个 entry。
- Engine `abi/engine` 或 `lib/engine` 变化重编 Engine、App 与 Hello Triangle，共 27 个 entry。
- ShaderToy lib 变化只重编自身 2 个 entry。
- Hello Triangle entry 变化只重编自身 1 个 entry。

运行时仍使用 entry 相对路径，并以 `app`、`sample-hello-triangle`、`sample-shader-toy` package id
解析 `build/shader` 产物。
