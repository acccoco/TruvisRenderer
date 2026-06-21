# Shader

`engine/shader/` 负责 Slang shader 源码管理、SPIR-V 编译与 Rust 绑定生成。

## 目录说明

- `entry/`：按 pass / sample 组织的 shader 入口源码
- `api/`：CPU/GPU 共享 ABI 与 shader-visible API 头文件；`api/common/` 保存共享结构和全局绑定定义，
  `api/pass/` 保存各 pass 的 push constants / binding set 声明，`api/mod.slangi` 是 Rust 绑定生成的聚合入口
- `lib/`：shader 侧复用库代码，例如采样、PBR、环境贴图、GBuffer、scene access 与 bindless 辅助逻辑
- `../../build/shader/`：编译产物目录（SPIR-V）
- `truvis-shader-build/`：shader 编译工具 crate
- `truvis-shader-binding/`：通过 bindgen 从 `ffi/rust_ffi.hpp` 和共享 Slang 头文件生成 Rust 绑定的 crate

## 工作流

1. 修改 `entry/`、`api/` 或 `lib/`
2. 执行 `just shader`
3. 再运行渲染示例

`just shader` 会先运行 `cargo run --bin shader-build` 生成 `build/shader/**/*.spv`，
再构建 `truvis-shader-binding`，让 Rust 侧绑定跟随共享结构更新。
`shader-build` 在 `build/shader/.state/` 记录 manifest：单个入口 shader 变化时只重编该入口；
`api/`、`lib/` 或 entry 下的 include 文件变化时保守重编全部入口。需要绕过 manifest 时执行
`just shader-force`。

## ABI 布局约束

`api/common/` 和 `api/pass/` 中的共享结构同时服务 Slang/SPIR-V、C++ bindgen 和 Rust `repr(C)`，
不能只用 C 侧自然布局判断 push constant 或 shader-visible buffer 的字段位置。`uint2` / `float2`
等 2 分量向量字段按 8 字节边界安排；如果前一个字段结束位置不是 8 字节对齐，必须在该向量字段前
插入显式 4 字节 padding，不能把 padding 放在向量字段后面补救。

新增或调整共享 ABI 时，优先参考 `api/pass/denoise_accum.slangi` 中先 padding、再放 `uint2 image_size`
的模式。修改后执行 `just shader` 或 `just shader-force`，必要时用 `spirv-dis` 检查 SPIR-V 中的
`OpMemberDecorate ... Offset` 是否与生成的 Rust binding 一致。

## 注意事项

- 共享结构变更会影响 Rust 绑定，需要重新执行 `just shader`。
- `api/mod.slangi` 是共享结构与 pass API 的聚合入口；新增 CPU/GPU 共享类型或 pass 契约时应放入 `api/common/`
  或 `api/pass/` 的明确归属文件，再由该入口统一暴露给 bindgen。
- 离线 RT 的 TLAS / single-frame output descriptor set 和 push constants 属于 `api/pass/offline_rt.slangi`；
  Rust 侧必须使用生成的 `gpu::offline_rt::*` ABI，不再手写镜像结构。
- 新 pass 建议复用已有全局描述符布局约定，避免新增碎片化绑定模型。
