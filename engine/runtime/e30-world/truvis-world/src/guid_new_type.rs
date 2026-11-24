use slotmap::new_key_type;

new_key_type! {
    /// `GameWorld` 内一次 scene import 请求的 runtime 身份。
    ///
    /// 该 handle 只属于 scene import 流程，用来让 Renderer 查询或消费 scene 导入结果。
    /// 它不等同于 `AssetLoadService` 内部的 scene load handle，也不表示已经写入 `SceneStore` 的
    /// live instance。
    pub struct SceneImportHandle;

    /// `SceneStore` 内 live instance 的 runtime 身份。
    ///
    /// 该 handle 只在 CPU scene 生命周期内标识一个 instance。渲染运行时会在
    /// prepare/sync 阶段把它映射到稳定的 GPU instance slot，二者不共享所有权或生命周期。
    pub struct MeshInstanceHandle;

    /// `SceneStore` 内 live light 的 runtime 身份。
    ///
    /// 该 handle 标识某一类 CPU 侧光源记录；point / spot / area light 分别存放在独立
    /// `SlotMap` 中，因此 handle 只应回到创建它的 light map 查询。光源上传、GPU buffer
    /// 布局和 shader 可见访问仍由 render-side `RenderWorld` / shader `GpuScene` ABI 负责。
    pub struct LightHandle;
}
