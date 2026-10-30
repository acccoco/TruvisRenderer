use slotmap::new_key_type;

// key 只标识创建它的 registry 内的 slot/generation；不能跨 registry/device 使用或延长 native 生命周期。
new_key_type! {
    pub struct GfxImageHandle;
    pub struct GfxBufferHandle;
    pub struct GfxImageViewHandle;
}
