use slotmap::new_key_type;

use ash::vk;

new_key_type! { pub struct AssetTextureHandle; }
new_key_type! { pub struct AssetMeshHandle; }

/// 解码后的纹理数据。
///
/// 这里的数据已经是上传友好的 CPU bytes，但还没有创建任何 GPU 资源。
#[derive(Debug)]
pub struct LoadedTextureBytes {
    pub pixels: Vec<u8>,
    pub extent: vk::Extent3D,
    pub format: vk::Format,
}

/// 资源加载状态机
///
/// 状态流转: Unloaded -> Loading -> Ready
///                        \-> Failed
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum LoadStatus {
    /// 初始状态，资源尚未请求加载
    Unloaded,
    /// IO 阶段：正在后台线程读取文件或进行 CPU 解码 (如 png -> rgba8)
    Loading,
    /// 完成状态：文件已经加载并解码为 CPU bytes
    Ready,
    /// 失败状态：文件不存在、格式错误或解码失败
    Failed,
}
