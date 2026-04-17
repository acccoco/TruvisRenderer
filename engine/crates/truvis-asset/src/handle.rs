use slotmap::new_key_type;
use truvis_render_interface::handles::{GfxImageHandle, GfxImageViewHandle};
use truvis_shader_binding::gpu;

new_key_type! { pub struct AssetTextureHandle; }
new_key_type! { pub struct AssetMeshHandle; }

/// 资产语义的纹理，而非 GPU 语义。
///
/// 聚合了纹理资产的完整描述（GPU 资源 handle + 采样/格式元信息），
/// 与 Bindless 系统刻意隔离——本类型不持有任何 `BindlessSrvHandle`，
/// bindless 索引的解析由外部（如 `TextureResolver`）在使用时按需完成。
#[derive(Debug)]
pub struct AssetTexture {
    pub image_handle: GfxImageHandle,
    pub view_handle: GfxImageViewHandle,
    pub sampler: gpu::ESamplerType,
    pub is_srgb: bool,
    pub mip_levels: u32,
}

/// 资源加载状态机
///
/// 状态流转: Unloaded -> Loading -> Uploading -> Ready
///                                  \-> Failed
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum LoadStatus {
    /// 初始状态，资源尚未请求加载
    Unloaded,
    /// IO 阶段：正在后台线程读取文件或进行 CPU 解码 (如 png -> rgba8)
    Loading,
    /// GPU 上传阶段：数据已提交到 Transfer Queue，正在等待 Timeline Semaphore 信号
    /// 此时资源在 GPU 上还不可用，但 CPU 端工作已完成
    Uploading,
    /// 完成状态：GPU 资源已完全就绪，可以绑定到 Descriptor Set 进行渲染
    Ready,
    /// 失败状态：文件不存在、格式错误或解码失败
    Failed,
}
