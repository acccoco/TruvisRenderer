# Shader

`engine/shader/` 负责 Slang shader 源码管理、SPIR-V 编译与 Rust 绑定生成。

## 目录说明

- `entry/`：按 pass 组织的 shader 入口源码（`.slang`）
- `share/`：共享头文件（`.slangi`），包含结构体与全局绑定定义
- `lib/`：shader 侧复用库代码，例如采样、PBR、环境贴图、GBuffer 与 bindless 辅助逻辑
- `.build/`：编译产物目录（SPIR-V）
- `truvis-shader-build/`：shader 编译工具 crate
- `truvis-shader-binding/`：通过 bindgen 从 `ffi/rust_ffi.hpp` 和共享 Slang 头文件生成 Rust 绑定的 crate

## 工作流

1. 修改 `entry/`、`share/` 或 `lib/`
2. 执行 `just shader`
3. 再运行渲染示例

`just shader` 会先运行 `cargo run --bin shader-build` 生成 `.build/**/*.spv`，
再构建 `truvis-shader-binding`，让 Rust 侧绑定跟随共享结构更新。

## 注意事项

- 共享结构变更会影响 Rust 绑定，需要重新执行 `just shader`。
- 新 pass 建议复用已有全局描述符布局约定，避免新增碎片化绑定模型。
