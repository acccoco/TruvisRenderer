use std::ffi::CString;

/// 执行一次 VMA 分配调用，并让 allocation user data 复制调试名称。
///
/// 临时 allocation info 设置了 `USER_DATA_COPY_STRING`，因此 VMA 会在回调执行期间复制字符串。
pub fn with_vma_debug_name<T>(
    base_info: &vk_mem::AllocationCreateInfo,
    debug_name: &str,
    create: impl FnOnce(&vk_mem::AllocationCreateInfo) -> T,
) -> T {
    let stable_name = CString::new(debug_name).unwrap_or_else(|_| {
        let sanitized = debug_name.replace('\0', "?");
        CString::new(sanitized).expect("sanitized VMA debug name should not contain interior NUL bytes")
    });

    let mut allocation_info = base_info.clone();
    #[allow(deprecated)]
    {
        allocation_info.flags |= vk_mem::AllocationCreateFlags::USER_DATA_COPY_STRING;
    }
    allocation_info.user_data = stable_name.as_ptr() as usize;

    create(&allocation_info)
}
