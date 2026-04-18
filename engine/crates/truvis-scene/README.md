# truvis-scene

CPU 侧场景数据模块，管理 mesh/material/instance/light 等实体数据。

## 核心职责

- 场景组件存储与检索
- 场景实例组织与引用关系维护
- 生成供渲染上传使用的数据视图

## 与渲染关系

- 负责 CPU 语义数据，不直接承担底层 GPU 执行逻辑
- 与 `truvis-render-interface` 的数据契约协作完成上传
