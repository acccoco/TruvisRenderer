pub struct BytesConvert {}
impl BytesConvert {
    pub fn bytes_of<T: Sized>(data: &T) -> &[u8] {
        let data_slice = std::slice::from_ref(data);
        unsafe { std::slice::from_raw_parts(data_slice as *const _ as *const u8, std::mem::size_of::<T>()) }
    }
}
