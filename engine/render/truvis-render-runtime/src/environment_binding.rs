use truvis_render_foundation::bindless_manager::BindlessSrvHandle;
use truvis_shader_binding::gpu;

/// scene root buffer 写入的 sky 绑定。
///
/// 该绑定只表达 shader 可读的 SRV 与采样器类型，不携带 sky 的加载状态或资源所有权。
#[derive(Clone, Copy)]
pub(crate) struct EnvironmentSkyBinding {
    pub(crate) srv_handle: BindlessSrvHandle,
    pub(crate) sampler: gpu::ESamplerType,
}

/// scene root buffer 写入的普通环境贴图绑定。
///
/// 当前用于 UV checker 这类 runtime 常驻辅助贴图；资源生命周期由具体 owner 管理。
#[derive(Clone, Copy)]
pub(crate) struct EnvironmentTextureBinding {
    pub(crate) srv_handle: BindlessSrvHandle,
    pub(crate) sampler: gpu::ESamplerType,
}

/// 本帧 GPU scene 使用的环境资源快照。
///
/// `GpuScene` 只消费该快照并写入 scene root buffer，不关心 sky 是否来自真实贴图、
/// fallback 贴图，或后续的 sky PDF 生成流程。
#[derive(Clone, Copy)]
pub(crate) struct EnvironmentBinding {
    pub(crate) sky: EnvironmentSkyBinding,
    pub(crate) uv_checker: EnvironmentTextureBinding,
}
