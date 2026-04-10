//! 着色器绑定布局 trait
//!
//! 配合 `truvis-descriptor-layout-macro` 宏，自动生成 Vulkan 描述符集布局。
//! 通过 `#[shader_layout]` 宏标注结构体，自动实现 [`DescriptorBindingLayout`] trait。
//!
//! # 使用示例
//! ```ignore
//! #[shader_layout]
//! struct MyLayout {
//!     #[binding = 0] uniforms: PerFrameData,
//!     #[texture(binding = 1)] diffuse: TextureHandle,
//!     #[sampler(binding = 2)] sampler: SamplerHandle,
//! }
//! ```

use ash::vk;

/// 描述符绑定的详细信息
#[derive(Debug, Clone, Copy)]
pub struct DescriptorBindingItem {
    pub name: &'static str,
    pub binding: u32,
    pub descriptor_type: vk::DescriptorType,
    pub stage_flags: vk::ShaderStageFlags,
    pub count: u32,
    pub flags: vk::DescriptorBindingFlags,
}

/// 着色器绑定布局 trait
///
/// 描述着色器需要的所有资源绑定。通过 `#[shader_layout]` 宏自动实现。
pub trait DescriptorBindingLayout {
    /// 获取所有绑定的详细信息
    ///
    /// 由宏自动实现，返回包含名称、绑定点、描述符类型、着色器阶段的数组。
    fn get_shader_bindings() -> Vec<DescriptorBindingItem>;

    /// 获取 Vulkan 描述符集布局绑定
    ///
    /// 不应被覆盖，使用 `get_shader_bindings()` 生成 Vulkan 所需的绑定信息。
    fn get_vk_bindings() -> (Vec<vk::DescriptorSetLayoutBinding<'static>>, Vec<vk::DescriptorBindingFlags>) {
        let bindings = Self::get_shader_bindings();
        let layout_bindings = bindings
            .iter()
            .map(|item| vk::DescriptorSetLayoutBinding {
                binding: item.binding,
                descriptor_type: item.descriptor_type,
                descriptor_count: item.count,
                stage_flags: item.stage_flags,
                ..Default::default()
            })
            .collect();

        let binding_flags = bindings.iter().map(|item| item.flags).collect();

        (layout_bindings, binding_flags)
    }
}
