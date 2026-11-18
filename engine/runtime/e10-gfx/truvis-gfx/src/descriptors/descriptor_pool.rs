use std::rc::Rc;

use ash::vk;
use ash::vk::Handle;

use crate::foundation::debug_messenger::DebugType;
use crate::gfx::GfxDeviceCtx;

/// 描述符池
///
/// 描述符池用于分配描述符集。
/// 一个 pool 可分配不同布局的 set；类型容量、set 数量和 GPU 使用期由 pool owner 保证。
pub struct GfxDescriptorPool {
    /// Vulkan 描述符池句柄
    handle: vk::DescriptorPool,
    /// 描述符池创建信息
    _info: Rc<GfxDescriptorPoolCreateInfo>,

    _name: String,
}
impl GfxDescriptorPool {
    /// 创建新的描述符池
    ///
    /// # 参数
    /// - render_context: RHI 实例
    /// - ci: 描述符池创建信息
    /// - name: 用于调试的名称
    ///
    /// # 返回值
    /// 新的描述符池实例
    #[inline]
    pub fn new(ctx: GfxDeviceCtx<'_>, ci: Rc<GfxDescriptorPoolCreateInfo>, name: &str) -> Self {
        let name = name.to_owned();
        let gfx_device = ctx.device();
        let pool = unsafe { gfx_device.create_descriptor_pool(&ci.inner, None).unwrap() };
        let pool = Self {
            handle: pool,
            _info: ci,
            _name: name,
        };
        gfx_device.set_debug_name(&pool, &pool._name);
        pool
    }

    /// 获取 Vulkan 描述符池句柄
    ///
    /// # 返回值
    /// Vulkan 描述符池句柄
    #[inline]
    pub fn handle(&self) -> vk::DescriptorPool {
        self.handle
    }

    #[inline]
    pub fn destroy(mut self, ctx: GfxDeviceCtx<'_>) {
        if self.handle.is_null() {
            return;
        }
        unsafe { ctx.device().destroy_descriptor_pool(self.handle, None) };
        self.handle = vk::DescriptorPool::null();
    }
}
impl Drop for GfxDescriptorPool {
    /// 仅诊断遗漏的显式释放，不在 Drop 调用 Vulkan。
    fn drop(&mut self) {
        debug_assert!(self.handle.is_null(), "GfxDescriptorPool '{}' dropped without explicit destroy", self._name);
    }
}
impl DebugType for GfxDescriptorPool {
    fn debug_type_name() -> &'static str {
        "GfxDescriptorPool"
    }

    fn vk_handle(&self) -> impl vk::Handle {
        self.handle
    }
}

/// 描述符池创建信息
///
/// 用于配置描述符池的创建参数，包括：
/// - 标志位
/// - 最大描述符集数量
/// - 每种类型描述符的最大数量
pub struct GfxDescriptorPoolCreateInfo {
    /// Vulkan 描述符池创建信息
    inner: vk::DescriptorPoolCreateInfo<'static>,
    /// 描述符池大小信息
    _pool_sizes: Vec<vk::DescriptorPoolSize>,
}

impl GfxDescriptorPoolCreateInfo {
    /// 创建新的描述符池创建信息
    ///
    /// # 参数
    /// - flags: 创建标志
    /// - max_sets: 最大描述符集数量
    /// - pool_sizes: 每种类型描述符的最大数量
    ///
    /// # 返回值
    /// 新的描述符池创建信息实例
    #[inline]
    pub fn new(flags: vk::DescriptorPoolCreateFlags, max_sets: u32, pool_sizes: Vec<vk::DescriptorPoolSize>) -> Self {
        let inner = vk::DescriptorPoolCreateInfo {
            flags,
            max_sets,
            pool_size_count: pool_sizes.len() as u32,
            p_pool_sizes: pool_sizes.as_ptr(),
            ..Default::default()
        };
        Self {
            inner,
            _pool_sizes: pool_sizes,
        }
    }
}
