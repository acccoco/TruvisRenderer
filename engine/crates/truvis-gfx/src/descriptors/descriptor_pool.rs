use std::rc::Rc;

use ash::vk;

use crate::foundation::debug_messenger::DebugType;
use crate::gfx::Gfx;

/// 描述符池
///
/// 描述符池用于分配描述符集。
/// 一个描述符池可以分配多个描述符集，但所有描述符集必须使用相同的布局。
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
    pub fn new(ci: Rc<GfxDescriptorPoolCreateInfo>, name: &str) -> Self {
        let gfx_device = Gfx::get().gfx_device();
        let pool = unsafe { gfx_device.create_descriptor_pool(&ci.inner, None).unwrap() };
        let pool = Self {
            handle: pool,
            _info: ci,
            _name: name.to_string(),
        };
        gfx_device.set_debug_name(&pool, name);
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
    pub fn destroy(self) {
        // Drop 处理
    }
}
impl Drop for GfxDescriptorPool {
    /// 释放 Vulkan 描述符池
    fn drop(&mut self) {
        unsafe { Gfx::get().gfx_device().destroy_descriptor_pool(self.handle, None) };
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
