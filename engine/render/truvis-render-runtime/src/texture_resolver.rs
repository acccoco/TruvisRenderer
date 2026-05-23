//! 材质准备阶段使用的纹理绑定解析接口。
//!
//! 本模块只定义 runtime 内部的窄契约：材质管理器通过 `TextureResolver`
//! 查询 asset texture 是否 ready，并取得 shader 可读取的 bindless binding。
//! 纹理上传、fallback 资源所有权和 bindless 注册仍由上传器等实现方负责。

use truvis_asset::handle::AssetTextureHandle;
use truvis_render_foundation::bindless_manager::BindlessSrvHandle;
use truvis_shader_binding::gpu;

#[derive(Clone, Copy)]
pub struct TextureBinding {
    pub srv_handle: BindlessSrvHandle,
    pub sampler: gpu::ESamplerType,
}

impl TextureBinding {
    /// 构造 shader 可安全读取的空 texture binding。
    ///
    /// 该值用于“材质没有贴图”场景；“贴图存在但未 ready”由 `TextureResolver`
    /// 返回 fallback binding 处理。
    pub fn null() -> Self {
        Self {
            srv_handle: BindlessSrvHandle::null(),
            sampler: gpu::ESamplerType_LinearRepeat,
        }
    }
}

/// 纹理 ready 状态与 shader binding 查询接口。
///
/// 由渲染侧纹理上传/绑定缓存实现，避免 scene 直接耦合 AssetHub 或 BindlessManager。
pub trait TextureResolver {
    /// texture 是否已经拥有真实 GPU image/view/bindless binding。
    fn is_texture_ready(&self, handle: AssetTextureHandle) -> bool;

    /// 获取可渲染的 texture binding；未就绪时由实现返回 fallback。
    fn resolve_texture(&self, handle: AssetTextureHandle) -> TextureBinding;
}
