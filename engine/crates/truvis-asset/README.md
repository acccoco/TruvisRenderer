# truvis-asset

资产加载模块，提供纹理等资源的异步文件读取、CPU 解码与状态管理能力。

本模块只负责到 upload-ready CPU bytes，不创建 GPU image / image view，
也不注册 bindless descriptor。GPU 上传和 shader 可见绑定由
`truvis-render-backend` 的 `AssetTextureUploader` 负责。

## 主要组件

- `AssetHub`：对外统一入口
- `AssetLoader`：后台 IO 与 CPU 解码
- `LoadedAssetEvent`：加载完成事件，交给渲染后端继续上传

## 设计目标

- 避免阻塞渲染主流程
- 通过状态机管理 Loading -> Ready / Failed 过程
- 保持 asset 层不依赖 GPU 资源缓存或 bindless 绑定策略
