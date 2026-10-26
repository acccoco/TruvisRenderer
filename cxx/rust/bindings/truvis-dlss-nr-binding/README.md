# truvis-dlss-nr-binding

单向消费 `truvixx-dlss-nr-capi` public header，复用 `truvis-cxx-binding-codegen` 生成类型和函数。
不手写 ABI 镜像，不导入 NGX C++ 类型，不从 CMake 调用 Cargo。仅面向 Windows x64 MSVC。

`NrSession` 用 `PhantomData<Rc<()>>` 限制为 `!Send + !Sync`，并且 native 端检查创建线程。
它只拥有 opaque NGX handle；调用方负责让 Vulkan root 与 Streamline runtime 保持存活并串行协调调用。

```text
extensions → init → capabilities → create_feature → evaluate（多帧）
GPU 完成 → release_feature → 可重新 create_feature
GPU 完成且所有 SR/RR/NR feature 释放 → shutdown → 关闭 Streamline → 销毁 Vulkan root
```

`create_feature/evaluate` 不提交 GPU 工作。显式 `release_feature` 只释放 feature，`shutdown` 才结束
NGX 会话；成功后清空 handle，失败保留它。`Drop` 只记录未显式关闭的错误并在 Debug 检查遗漏，不隐式调用
NGX 或 Vulkan。不要依赖 Rust 字段析构顺序替代上层 owner 的显式关闭流程。

涉及 raw Vulkan handle、函数指针、UTF-16 指针及借用图像的方法均为 `unsafe`，各方法的 Safety 注释
给出调用前置条件。`capabilities` 返回的每个查询结果必须单独检查。`NrError` 保留失败阶段、NGX 原始值
和 wrapper 错误，不把能力查询成功等同于可执行。

图像格式、颜色域、设备 feature 和部署约束以 [native 模块 README](../../../modules/truvixx-dlss-nr/README.md)
及唯一 public header 为准。构建先运行 `just cxx`，再使用普通 Cargo build；binding 只链接已部署的
profile 对应 import library。阶段 2 的临时 Rust 验证入口位于 `.temp/dlss5-validation/stage2/host/`，
通过本 crate 执行 NR，GPU fixture 只提供应用图像、command buffer、SR/RR 与同步设施。

正式 Renderer 的 consumer 是 `renderer-rendering::NrSubsystem`，由 `TruvisRenderer` 持有。
资源回调负责图像分配与 create 提交/等待；关闭效果保留会话，最终先释放所有 SR/RR/NR feature 再关闭 NR。
当前只有状态和资源集成，没有 evaluate 或显示接线。生命周期与配置边界见
[Renderer README](../../../../renderer/truvis-renderer/README.md)。
