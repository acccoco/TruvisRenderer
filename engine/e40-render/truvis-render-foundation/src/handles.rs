use slotmap::new_key_type;

// 内部 Key (不直接暴露，或者作为底层 API)
new_key_type! {
    pub struct GfxImageHandle;
    pub struct GfxBufferHandle;
    pub struct GfxImageViewHandle;
}
