# Renderer Shader

`renderer/shader/` 是所有 Renderer 侧 shader 源码和 Renderer Rust binding 的唯一物理根目录。Rust crate 仍按
`renderer-render-passes`、`renderer-imgui`、`renderer-rendering` 和 sample 分工，但 shader package 按相同依赖、include roots、增量失效
范围和发布边界组织，不跟随 Rust crate 目录拆散。

## 目录职责

```text
renderer/shader/
├─ abi/
│  └─ renderer/
│     ├─ mod.slangi
│     ├─ render_passes/
│     └─ kit/
├─ lib/
│  ├─ renderer/
│  └─ sample-shader-toy/
├─ entry/
│  ├─ renderer/
│  ├─ sample-hello-triangle/
│  └─ sample-shader-toy/
└─ truvis-renderer-shader-binding/
```

- `abi/renderer/` 是 Rust / Slang 共享契约，保持 `renderer::render_passes::*` 与
  `renderer::kit::*` namespace；统一聚合入口是 `abi/renderer/mod.slangi`。
- ImGui backend 虽然属于 Rust crate `renderer-imgui`，现有 GUI ABI 仍保持 `renderer::kit::ui_imgui`；crate 边界不决定 shader namespace。
- `lib/renderer/` 保存依赖 Renderer ABI/resource 的 RT、ReSTIR、SHARC、GBuffer、raster 与环境贴图
  算法，只在 Slang 侧共享，不生成 Rust binding。
- `lib/sample-shader-toy/` 只对 ShaderToy 可见，不得引用 Engine 或 Renderer。
- `entry/` 只保存最终编译入口，不作为其它 shader 的共享接口，也不生成 Rust binding。
- `truvis-renderer-shader-binding/` 是全部 Renderer ABI 的唯一 Rust binding owner，正向依赖
  `truvis-shader-binding`。

## 四个 package

| package | entry root | 可见 include roots | 依赖 |
| --- | --- | --- | --- |
| `engine` | `engine/shader/entry` | `engine/shader` | 无 |
| `renderer` | `entry/renderer` | `engine/shader`、`renderer/shader` | `engine` |
| `sample-hello-triangle` | `entry/sample-hello-triangle` | Engine | `engine` |
| `sample-shader-toy` | `entry/sample-shader-toy` | `lib/sample-shader-toy` | 无 |

manifest 不提供 workspace 根目录 `.` include root。搜索根与 `.vscode/settings.json` 保持一致，构建器不读取
编辑器配置。include 必须从搜索根开始并带 owner：Renderer 使用 `abi/renderer/...`、`lib/renderer/...`，Engine 使用
`abi/engine/...`、`lib/engine/...`，ShaderToy 使用 `lib/sample-shader-toy/...`。每个路径必须唯一解析，
禁止 `../`、绝对路径、反斜杠和依赖搜索顺序的裸 domain 路径。

依赖层级固定为 `abi/<owner> <- lib/<owner> <- entry`；ABI 不能依赖 lib/entry，lib 不能依赖 entry。
当前 manifest 声明的 Owner 方向为 `engine <- renderer`，通用构建器只认 package 依赖闭包和带层级的
`shared_inputs`，不内置 Engine、Renderer 或 sample 名称。构建器先静态检查源码，再用 Slang/GLSLC/DXC depfile 验证实际传递依赖；
ShaderToy 即使物理上位于 `renderer/shader`，也不能访问 Renderer 或 Engine shared inputs。

## Binding 唯一所有权

### 材质映射与几何 ABI

Engine 的 `MaterialTexture` 为 48 字节（两行 float4，随后 SRV、sampler、UV 集和 padding），
`PbrMaterial` 为 256 字节，四槽数组从 offset 64 开始。`Geometry` 为 56 字节，包含 vertex_count、
uv_set_count、tangent_tex_coord；UV 集连续存储，以 vertex_count 为集合 stride，tangent 为 float4。
vertex SoA 的 tangent 起点按 16 字节对齐，不把紧凑 float3 当成 16 字节数据。
`EmissiveTriangleLight` 为 80 字节，仅保存几何身份和采样所需几何信息，UV 由几何表按槽查询。
Phong 的 push constants 为 16 字节，字段 offset 为 0/4/8/12；frame/scene root 使用全局 descriptor，
不再传入 UBO device address。唯一顶点入口为 `raster/phong3d.vs.slang`，复用 `PhongVertex` 读取各槽 UV。

生成的 host binding 使用自身自然对齐（MaterialTexture/PbrMaterial 为 4，Geometry 为 8），
上传 buffer/device address 满足 shader 的访问对齐；字段 offset 和数组 stride 必须与 SPIR-V 一致。
2026-09-17 已对照生成 binding 与 `spirv-dis`，并通过当前全部 28 个 shader 的
`spirv-val --uniform-buffer-standard-layout`。新增/调整字段仍需重新生成和检查。

realtime/offline 的 closest-hit 入口共用 `lib/renderer/realtime_rt/surface_hit.slangi`；
raster 顶点侧按四槽分别选择 UV，片元侧与 RT 共用 Engine `MaterialAccess` 的变换、切线重建和法线解码。
各路径的光照和 LOD 边界见 [RT 流程](../../docs/design/realtime-raytracing-sampling.md)。

### 生成绑定

`truvis-renderer-shader-binding` 只 allowlist `renderer::*`，generator 固定
`allowlist_recursively(false)`。canonical Slang 基础类型从 `truvis-shader-binding` 显式 re-export；未来
Renderer ABI 若引用具名 Engine 类型，必须通过 `ModuleRawLine` 注入到 bindgen 实际生成的 Engine namespace。
缺失映射时允许 Rust 编译失败，以 fail-closed 暴露依赖错误。

Renderer 的 header、package include roots、额外 Engine FFI include root 与输出 crate 由
`shader-packages.toml` 的 `[[binding]] id = "renderer"` 声明；owner `build.rs` 不再拼接 shader 目录。

Engine 类型只由 `truvis-shader-binding` 定义；不使用禁止定义黑名单，也不为旧命名的 Renderer binding crate 或旧
shader 路径保留兼容层。

## 增量边界

- `renderer/shader/abi/renderer` 或 `lib/renderer` 变化只重编 Renderer 的 21 个 entry。
- Engine `abi/engine` 或 `lib/engine` 变化重编 Engine、Renderer 与 Hello Triangle，共 26 个 entry。
- ShaderToy lib 变化只重编自身 2 个 entry。
- Hello Triangle entry 变化只重编自身 1 个 entry。

运行时仍使用 entry 相对路径，并通过 `ShaderArtifactPath::resolve` 以 `renderer`、
`sample-hello-triangle`、`sample-shader-toy` package id 解析产物。物理根来自 `paths.toml` 的
`dirs.shader_build`，package id 到输出前缀的映射来自 `shader-packages.toml`。
