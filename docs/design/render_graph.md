# RenderGraph 核心设计思路

## 关键设计

- 两层 Handle：`RgImageHandle`（虚拟）→ `GfxImageHandle`（物理）
- RenderGraph 一定不需要关心 Texture 等资产概念，完全基于 GPU 资源（Image/Buffer）进行构建

## 核心流程

### 资源声明

- 通过 import/export 的方式依赖外部的资源，并且指定同步原语（`AccessFlags` + `PipelineStageFlags`）
- 将 `GfxImageHandle` 注册到 RenderGraph，得到 `RgImageHandle` 供 Pass 使用


### Pass 添加：声明依赖

- **Pass 添加顺序非常重要**：这是渲染管线的**逻辑顺序**，由用户决定
- 每个 Pass 通过 `setup()` 声明资源依赖：`read_image(handle, state)` / `write_image(handle, state)`

### 依赖图构建：模拟资源访问

**方法**：模拟 Pass 添加顺序，跟踪资源访问，建立依赖边

```
图结构:  Node = Pass,  Edge = 资源依赖 (images[], buffers[])
```

**边建立规则**：维护 `last_writer[resource] = pass_idx`

| 依赖类型 | 规则 |
|---------|------|
| 写后读 (RAW) | Reader 依赖 Writer |
| 写后写 (WAW) | 后 Writer 依赖前 Writer |

### 拓扑排序：确定执行顺序

- 对 DAG 执行拓扑排序
- 检测循环依赖（有环则 panic）
- 保证 Producer 在 Consumer 之前

### Barrier 计算：模拟命令提交

**方法**：按拓扑排序后的顺序，跟踪资源状态变化，在状态转换处插入 barrier

```
for pass in execution_order:
    for (resource, required_state) in pass.resources:
        current_state = state_tracker[resource]
        if needs_barrier(current_state, required_state):
            emit_barrier(current → required)
        state_tracker[resource] = required_state
```

**Barrier 判断**：
- Layout 不同 → barrier
- 有写操作 → barrier
- 只读→只读 + 相同 layout → 跳过
