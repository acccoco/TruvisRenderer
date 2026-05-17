use slotmap::new_key_type;

new_key_type! {
    /// `SceneManager` 内 live instance 的 runtime 身份。
    ///
    /// 该 handle 只在 CPU scene 生命周期内标识一个 instance。渲染后端会在
    /// prepare/sync 阶段把它映射到稳定的 GPU instance slot，二者不共享所有权或生命周期。
    pub struct InstanceHandle;

    /// `SceneManager` 内 live point light 的 runtime 身份。
    ///
    /// 该 handle 标识 CPU 侧光源记录。光源上传、GPU buffer 布局和 shader 可见访问仍由
    /// render-side scene bridge / `GpuScene` 负责。
    pub struct LightHandle;
}
