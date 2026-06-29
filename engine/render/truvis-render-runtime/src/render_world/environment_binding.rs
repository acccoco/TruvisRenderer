use truvis_shader_binding::gpu;

use crate::bindings::bindless_manager::BindlessSrvHandle;

/// scene root buffer 写入的 sky 绑定。
///
/// 该绑定只表达 shader 可读的 SRV 与采样器类型，不携带 sky 的加载状态或资源所有权。
#[derive(Clone, Copy)]
pub(crate) struct EnvironmentSkyBinding {
    pub(crate) srv_handle: BindlessSrvHandle,
    pub(crate) sampler: gpu::bindless::ESamplerType,
    pub(crate) distribution_device_address: u64,
    pub(crate) distribution_width: u32,
    pub(crate) distribution_height: u32,
    pub(crate) distribution_enabled: u32,
    pub(crate) distribution_version: u32,
}

/// 本帧 GPU scene 使用的环境资源快照。
///
/// `RenderWorld` 只消费该快照并写入 scene root buffer，不关心 sky 是否来自真实贴图、
/// fallback 贴图，或后续的 sky PDF 生成流程。
#[derive(Clone, Copy)]
pub(crate) struct EnvironmentBinding {
    pub(crate) sky: EnvironmentSkyBinding,
}
