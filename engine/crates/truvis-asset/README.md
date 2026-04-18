# truvis-asset

资产加载模块，提供纹理等资源的异步加载、上传与状态管理能力。

## 主要组件

- `AssetHub`：对外统一入口
- `AssetLoader`：后台 IO 与解码
- `AssetUploadManager`：GPU 上传与同步推进

## 设计目标

- 避免阻塞渲染主流程
- 通过状态机管理 Loading -> Uploading -> Ready 过程
