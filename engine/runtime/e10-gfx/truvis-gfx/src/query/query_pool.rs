use ash::vk;
use ash::vk::Handle;

use crate::{foundation::debug_messenger::DebugType, gfx::GfxDeviceCtx};

/// 每个 query 返回一个 u64 标量；多计数器/可变布局查询需要由相应 owner 单独建模。
pub struct GfxQueryPool {
    handle: vk::QueryPool,
    query_type: vk::QueryType,

    /// pool 的容量
    count: u32,
    debug_name: String,
}
impl DebugType for GfxQueryPool {
    fn debug_type_name() -> &'static str {
        "GfxQueryPool"
    }

    fn vk_handle(&self) -> impl vk::Handle {
        self.handle
    }
}
impl Drop for GfxQueryPool {
    fn drop(&mut self) {
        debug_assert!(self.handle.is_null(), "GfxQueryPool '{}' dropped without explicit destroy", self.debug_name);
    }
}
impl GfxQueryPool {
    #[inline]
    pub fn new(ctx: GfxDeviceCtx<'_>, ty: vk::QueryType, cnt: u32, debug_name: &str) -> Self {
        assert!(cnt > 0, "query pool capacity must be nonzero");
        assert!(
            matches!(ty, vk::QueryType::OCCLUSION | vk::QueryType::TIMESTAMP)
                || Self::is_acceleration_structure_query(ty),
            "GfxQueryPool only supports single-scalar query results"
        );
        let gfx_device = ctx.device();
        let debug_name = debug_name.to_owned();
        let create_info = vk::QueryPoolCreateInfo {
            query_type: ty,
            query_count: cnt,
            ..Default::default()
        };

        let handle = unsafe { gfx_device.create_query_pool(&create_info, None).unwrap() };

        let query_pool = Self {
            handle,
            query_type: ty,
            count: cnt,
            debug_name,
        };
        gfx_device.set_debug_name(&query_pool, &query_pool.debug_name);
        query_pool
    }

    #[inline]
    pub fn handle(&self) -> vk::QueryPool {
        self.handle
    }

    #[inline]
    pub fn query_type(&self) -> vk::QueryType {
        self.query_type
    }

    #[inline]
    /// WAIT 只等待对应 query 可用；owner 仍须保证 reset/reuse 顺序，不能将其视为整批 GPU 完成。
    pub fn get_query_result(&mut self, ctx: GfxDeviceCtx<'_>, first_index: u32, query_cnt: u32) -> Vec<u64> {
        self.validate_range(first_index, query_cnt);
        if query_cnt == 0 {
            return Vec::new();
        }
        let gfx_device = ctx.device();
        unsafe {
            let mut res = vec![0u64; query_cnt as usize];
            gfx_device
                .get_query_pool_results(
                    self.handle,
                    first_index,
                    &mut res,
                    vk::QueryResultFlags::TYPE_64 | vk::QueryResultFlags::WAIT,
                )
                .unwrap();
            res
        }
    }

    #[inline]
    pub fn reset(&mut self, ctx: GfxDeviceCtx<'_>, first_query: u32, query_cnt: u32) {
        self.validate_range(first_query, query_cnt);
        if query_cnt == 0 {
            return;
        }
        let gfx_device = ctx.device();
        unsafe {
            gfx_device.reset_query_pool(self.handle, first_query, query_cnt);
        }
    }

    fn validate_range(&self, first: u32, count: u32) {
        assert!(first.checked_add(count).is_some_and(|end| end <= self.count), "query range exceeds pool capacity");
    }

    pub(crate) fn validate_acceleration_write_range(&self, first: u32, count: u32) {
        assert!(Self::is_acceleration_structure_query(self.query_type), "query pool is not an AS property pool");
        self.validate_range(first, count);
    }

    fn is_acceleration_structure_query(ty: vk::QueryType) -> bool {
        matches!(
            ty,
            vk::QueryType::ACCELERATION_STRUCTURE_COMPACTED_SIZE_KHR
                | vk::QueryType::ACCELERATION_STRUCTURE_SERIALIZATION_SIZE_KHR
                | vk::QueryType::ACCELERATION_STRUCTURE_SIZE_KHR
                | vk::QueryType::ACCELERATION_STRUCTURE_SERIALIZATION_BOTTOM_LEVEL_POINTERS_KHR
        )
    }

    #[inline]
    pub fn destroy(mut self, ctx: GfxDeviceCtx<'_>) {
        if self.handle.is_null() {
            return;
        }
        unsafe {
            ctx.device().destroy_query_pool(self.handle, None);
        }
        self.handle = vk::QueryPool::null();
    }
}
