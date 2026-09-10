# Asset Upload 与 Scene 能力演进

## 目标

在不改变 CPU scene 到 render-side prepared scene 主边界的前提下，提升大量资源加载、首次上传和场景生命周期管理能力。
目标是解决上传尖峰、失败恢复、资源卸载和细粒度 invalidation。资源 owner、RenderWorld 镜像和版本对账的目标模型由
[`render-scene-mirror-and-resource-system.md`](render-scene-mirror-and-resource-system.md) 定义；本文只承接该模型上的上传调度与生命周期能力。

## 当前基线

当前 CPU scene、asset loader、render-side managers 和 prepare 边界见
[`../summaries/scene-data-lifecycle.md`](../summaries/scene-data-lifecycle.md) 和
[`../summaries/render-graph-and-data-flow.md`](../summaries/render-graph-and-data-flow.md)。

## 待推进内容

- 上传批处理：把同一帧 ready 的 texture / mesh upload 合并到更少 command buffer 和 submit，
  保持每个 resource 的完成状态、失败状态和销毁 owner 清楚。
- Staging thread：评估把 staging buffer 准备、copy command 录制和 transfer queue submit 移出 render thread；
  render thread 只轮询完成并注册可见状态。
- 首次上传与失败恢复：在资源发布版本和镜像对账契约上完善 texture / mesh / material 的 ready gate、fallback、失败重试和旧资源延迟释放策略；mesh / texture 内容替换不属于首期范围。
- 资源卸载：评估 asset 引用索引、scene lifetime 和 unload，避免 live instance 或 sky/material 仍引用时释放底层资源。
- Strict readiness 策略：评估是否允许某些模式下禁用 fallback，要求 material / instance 等依赖全部 ready 后才可见。

## 边界与非目标

- 不让 asset loader 创建 Vulkan 对象或直接注册 shader-visible binding。
- 不让 render pass 读取 CPU scene owner、loader state 或 upload queue。
- 不在第一轮做完整 asset database 或跨项目资源包系统。
- 不改变失败 edit 不提交状态或推进 source revision、删除前检查依赖等 CPU scene 编辑语义；scene 导入本身不要求事务回滚。

## 完成标准

- 大量资源同帧完成时，上传提交次数和 render thread 尖峰可观察下降。
- 迟到上传、删除和失败路径都有明确 owner 和日志诊断。
- 资源卸载不会让旧 GPU slot / bindless handle 在飞命令中错误复用。
- 新增事实沉淀到 scene/data-flow/lifecycle summaries，本文件只保留后续能力。
