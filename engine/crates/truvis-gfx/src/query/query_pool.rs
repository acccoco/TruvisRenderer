use ash::vk;
use ash::vk::Handle;

use crate::{foundation::debug_messenger::DebugType, gfx::GfxDeviceCtx};

pub struct GfxQueryPool {
    handle: vk::QueryPool,
    query_type: vk::QueryType,

    /// pool 的容量
    _cnt: u32,
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
        let gfx_device = ctx.device();
        let create_info = vk::QueryPoolCreateInfo {
            query_type: ty,
            query_count: cnt,
            ..Default::default()
        };

        let handle = unsafe { gfx_device.create_query_pool(&create_info, None).unwrap() };

        let query_pool = Self {
            handle,
            query_type: ty,
            _cnt: cnt,
            debug_name: debug_name.to_string(),
        };
        gfx_device.set_debug_name(&query_pool, debug_name);
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
    pub fn get_query_result<T: Default + Sized + Clone>(
        &mut self,
        ctx: GfxDeviceCtx<'_>,
        first_index: u32,
        query_cnt: u32,
    ) -> Vec<T> {
        let gfx_device = ctx.device();
        unsafe {
            let mut res = vec![Default::default(); query_cnt as usize];
            gfx_device.get_query_pool_results(self.handle, first_index, &mut res, vk::QueryResultFlags::WAIT).unwrap();
            res
        }
    }

    #[inline]
    pub fn reset(&mut self, ctx: GfxDeviceCtx<'_>, first_query: u32, query_cnt: u32) {
        let gfx_device = ctx.device();
        unsafe {
            gfx_device.reset_query_pool(self.handle, first_query, query_cnt);
        }
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
