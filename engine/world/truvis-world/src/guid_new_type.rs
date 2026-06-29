use slotmap::new_key_type;

new_key_type! {
    /// `World` 内一次 model import 请求的 runtime 身份。
    ///
    /// 该 handle 只属于 scene asset ingest 流程，用来让 App 查询或消费 model 导入结果。
    /// 它不等同于 `AssetHub` 内部的 model asset handle，也不表示已经写入 `SceneStore` 的
    /// live instance。
    pub struct SceneModelImportHandle;

    /// `SceneStore` 内 live instance 的 runtime 身份。
    ///
    /// 该 handle 只在 CPU scene 生命周期内标识一个 instance。渲染运行时会在
    /// prepare/sync 阶段把它映射到稳定的 GPU instance slot，二者不共享所有权或生命周期。
    pub struct InstanceHandle;

    /// `SceneStore` 内 texture 语义记录的 runtime 身份。
    ///
    /// 该 handle 是 CPU scene 对 texture 的长期引用边界，不表示 texture CPU bytes 或
    /// GPU image 已存在。`AssetHub` 的 loader handle 只在 `SceneAssetIngestor` 内部用于
    /// 完成事件翻译，不会作为 scene / render-world 的长期身份。
    pub struct SceneTextureHandle;

    /// `SceneStore` 内 mesh 语义记录的 runtime 身份。
    ///
    /// 该 handle 是 CPU scene 对 mesh 的长期引用边界，不表示 mesh 已完成 GPU 上传或
    /// BLAS 构建。渲染侧 manager 以该 handle 作为 GPU-ready mesh cache key。
    pub struct SceneMeshHandle;

    /// `SceneStore` 内 material 语义记录的 runtime 身份。
    ///
    /// 该 handle 是 CPU scene 对材质参数的长期引用边界，不表示已经分配 shader 可见
    /// material slot。渲染侧 material manager 以该 handle 作为 scene-facing 注册 key。
    pub struct SceneMaterialHandle;

    /// `SceneStore` 内 live light 的 runtime 身份。
    ///
    /// 该 handle 标识某一类 CPU 侧光源记录；point / spot / area light 分别存放在独立
    /// `SlotMap` 中，因此 handle 只应回到创建它的 light map 查询。光源上传、GPU buffer
    /// 布局和 shader 可见访问仍由 render-side `RenderWorld` / shader `GpuScene` ABI 负责。
    pub struct LightHandle;
}
