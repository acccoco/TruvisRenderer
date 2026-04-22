use ash::vk;

use truvis_descriptor_layout_trait::DescriptorBindingLayout;

use crate::gfx::Gfx;
use crate::{descriptors::descriptor_pool::GfxDescriptorPool, foundation::debug_messenger::DebugType};

/// 描述符集布局
///
/// 描述符集布局定义了描述符集的结构，包括：
/// - 绑定的数量
/// - 每个绑定的类型
/// - 每个绑定的着色器阶段
///
/// 使用泛型参数 T 来关联具体的绑定布局类型，
/// 这样可以保证类型安全，并且可以在编译时检查布局的正确性。
///
/// # 泛型参数
/// - T: 实现了 ShaderBindingLayout trait 的类型，定义了具体的绑定布局
pub struct GfxDescriptorSetLayout<T: DescriptorBindingLayout> {
    /// Vulkan 描述符集布局句柄
    layout: vk::DescriptorSetLayout,
    /// 用于在编译时关联泛型参数 T
    phantom_data: std::marker::PhantomData<T>,
}
impl<T: DescriptorBindingLayout> GfxDescriptorSetLayout<T> {
    /// 创建新的描述符集布局
    ///
    /// # 参数
    /// - render_context: RHI 实例
    /// - debug_name: 用于调试的名称
    ///
    /// # 返回值
    /// 新的描述符集布局实例
    pub fn new(flags: vk::DescriptorSetLayoutCreateFlags, debug_name: impl AsRef<str>) -> Self {
        // 从类型 T 获取绑定信息
        let (bindings, binding_flags) = T::get_vk_bindings();
        let mut bind_flags_ci = vk::DescriptorSetLayoutBindingFlagsCreateInfo::default().binding_flags(&binding_flags);

        let create_info =
            vk::DescriptorSetLayoutCreateInfo::default().flags(flags).bindings(&bindings).push_next(&mut bind_flags_ci);
        vk::DescriptorBindingFlags::empty();

        let gfx_device = Gfx::get().gfx_device();
        // 创建 Vulkan 描述符集布局
        let layout = unsafe { gfx_device.create_descriptor_set_layout(&create_info, None).unwrap() };
        let layout = Self {
            layout,
            phantom_data: std::marker::PhantomData,
        };
        gfx_device.set_debug_name(&layout, debug_name);
        layout
    }

    #[inline]
    pub fn handle(&self) -> vk::DescriptorSetLayout {
        self.layout
    }

    #[inline]
    pub fn destroy(self) {
        // drop
    }
}
impl<T: DescriptorBindingLayout> Drop for GfxDescriptorSetLayout<T> {
    fn drop(&mut self) {
        unsafe {
            Gfx::get().gfx_device().destroy_descriptor_set_layout(self.layout, None);
        }
    }
}
impl<T: DescriptorBindingLayout> DebugType for GfxDescriptorSetLayout<T> {
    fn debug_type_name() -> &'static str {
        "GfxDescriptorSetLayout"
    }

    fn vk_handle(&self) -> impl vk::Handle {
        self.layout
    }
}

/// 描述符集
///
/// 描述符集是描述符的集合，用于在着色器中访问资源。
/// 每个描述符集都关联一个描述符集布局，定义了其结构。
///
/// # 泛型参数
/// - T: 实现了 ShaderBindingLayout trait 的类型，定义了具体的绑定布局
///
/// # Destroy
///
/// 跟随 descriptor pool 一起销毁
pub struct GfxDescriptorSet<T: DescriptorBindingLayout> {
    /// Vulkan 描述符集句柄
    handle: vk::DescriptorSet,
    /// 用于在编译时关联泛型参数 T
    phantom_data: std::marker::PhantomData<T>,

    _descriptor_pool: vk::DescriptorPool,
}
impl<T: DescriptorBindingLayout> GfxDescriptorSet<T> {
    /// 创建新的描述符集
    ///
    /// # 参数
    /// - render_context: RHI 实例
    /// - layout: 描述符集布局
    /// - debug_name: 用于调试的名称
    ///
    /// # 返回值
    /// 新的描述符集实例
    pub fn new(
        descriptor_pool: &GfxDescriptorPool,
        layout: &GfxDescriptorSetLayout<T>,
        debug_name: impl AsRef<str>,
    ) -> Self {
        // 分配描述符集
        let alloc_info = vk::DescriptorSetAllocateInfo::default()
            .descriptor_pool(descriptor_pool.handle())
            .set_layouts(std::slice::from_ref(&layout.layout));
        let gfx_device = Gfx::get().gfx_device();
        let descriptor_set = unsafe { gfx_device.allocate_descriptor_sets(&alloc_info).unwrap()[0] };
        let set = Self {
            handle: descriptor_set,
            phantom_data: std::marker::PhantomData,
            _descriptor_pool: descriptor_pool.handle(),
        };
        gfx_device.set_debug_name(&set, debug_name);
        set
    }

    #[inline]
    pub fn handle(&self) -> vk::DescriptorSet {
        self.handle
    }
}
impl<T: DescriptorBindingLayout> Drop for GfxDescriptorSet<T> {
    fn drop(&mut self) {
        // 无需手动释放，会跟随 DescriptorPool 一起释放
    }
}
impl<T: DescriptorBindingLayout> DebugType for GfxDescriptorSet<T> {
    fn debug_type_name() -> &'static str {
        "GfxDescriptorSet"
    }

    fn vk_handle(&self) -> impl vk::Handle {
        self.handle
    }
}

/// 描述符更新信息
///
/// 用于更新描述符集的内容，可以是：
/// - 图像描述符：用于纹理和采样器
/// - 缓冲区描述符：用于统一缓冲区和存储缓冲区
pub enum GfxDescriptorUpdateInfo {
    /// 图像描述符信息
    Image(vk::DescriptorImageInfo),
    /// 缓冲区描述符信息
    Buffer(vk::DescriptorBufferInfo),
}
