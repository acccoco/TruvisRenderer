use ash::vk;

use truvis_descriptor_layout_macro::DescriptorBinding;
use truvis_descriptor_layout_trait::DescriptorBindingLayout;

/// 示例：使用 DescriptorBinding 派生宏定义着色器布局
///
/// 这个结构体定义了着色器需要的所有资源绑定：
/// - 统一缓冲区 (Uniform Buffer)
/// - 纹理采样器 (Combined Image Sampler)
/// - 独立采样器 (Sampler)
#[derive(DescriptorBinding)]
struct MyShader {
    /// 统一缓冲区，绑定到绑定点 0
    /// 用于顶点和片段着色器
    #[binding = 0]
    #[descriptor_type = "UNIFORM_BUFFER"]
    #[count = 1]
    #[stage = "VERTEX | FRAGMENT"]
    _uniform_buffer_: (),

    /// 纹理采样器，绑定到绑定点 1
    /// 仅用于片段着色器
    #[binding = 1]
    #[descriptor_type = "UNIFORM_BUFFER_DYNAMIC"]
    #[count = 1]
    #[stage = "FRAGMENT"]
    _texture: (),

    /// 独立采样器，绑定到绑定点 2
    /// 仅用于片段着色器
    #[binding = 2]
    #[descriptor_type = "SAMPLER"]
    #[count = 1]
    #[stage = "FRAGMENT"]
    _sampler___: (),
}

fn main() {
    // 获取完整的绑定信息，包括描述符类型、数量和着色器阶段
    let binding_items = <MyShader as DescriptorBindingLayout>::get_shader_bindings();
    println!("Shader binding items: {:?}", binding_items);
}
