//! 着色器绑定布局 trait
//!
//! 配合 `truvis-descriptor-layout-macro` 宏，自动生成 Vulkan 描述符集布局。
//! 通过 `#[derive(DescriptorBinding)]` 为具名字段结构体实现 [`DescriptorBindingLayout`]。
//! 每个字段必须声明唯一的整数 binding；可选属性省略时使用默认值，非法属性在编译时拒绝。
//! 此投影不反射 shader，set 编号、资源类型、数量和 stage 仍须与 Shader ABI 对照。
//!
//! # 使用示例
//! ```ignore
//! use ash::vk;
//! use truvis_descriptor_layout_macro::DescriptorBinding;
//!
//! #[derive(DescriptorBinding)]
//! struct MyLayout {
//!     #[binding = 0] _uniforms: (),
//!     #[binding = 1]
//!     #[descriptor_type = "SAMPLED_IMAGE"]
//!     #[stage = "FRAGMENT"]
//!     _diffuse: (),
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
/// 描述一个 set 内的资源绑定。通过 `#[derive(DescriptorBinding)]` 自动实现。
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
