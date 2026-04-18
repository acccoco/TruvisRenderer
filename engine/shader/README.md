# Shader

`engine/shader/` 负责 Slang shader 源码管理、SPIR-V 编译与 Rust 绑定生成。

## 目录说明

- `entry/`：按 pass 组织的 shader 入口源码（`.slang`）
- `share/`：共享头文件（`.slangi`），包含结构体与全局绑定定义
- `.build/`：编译产物目录（SPIR-V）
- `truvis-shader-build/`：shader 编译工具 crate
- `truvis-shader-binding/`：自动生成的 Rust 绑定 crate

## 工作流

1. 修改 `entry/` 或 `share/`
2. 执行 `cargo run --bin shader-build`
3. 再运行渲染示例

## 注意事项

- 共享结构变更可能影响 Rust 绑定，需要完整重编译。
- 新 pass 建议复用已有全局描述符布局约定，避免新增碎片化绑定模型。
