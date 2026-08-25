use ash::vk;

/// 索引类型 Trait (u16 或 u32)
pub trait GfxIndexType: Sized + Copy {
    const VK_INDEX_TYPE: vk::IndexType;
    fn byte_size() -> usize;
}

impl GfxIndexType for u16 {
    const VK_INDEX_TYPE: vk::IndexType = vk::IndexType::UINT16;
    fn byte_size() -> usize {
        size_of::<u16>()
    }
}

impl GfxIndexType for u32 {
    const VK_INDEX_TYPE: vk::IndexType = vk::IndexType::UINT32;
    fn byte_size() -> usize {
        size_of::<u32>()
    }
}

/// Vertex Buffer 中顶点布局的 trait 定义
///
/// 定义了顶点数据的内存布局，包括 Binding 和 Attribute 描述。
/// 支持 AoS (Array of Structures) 和 SoA (Structure of Arrays) 布局。
pub trait GfxVertexLayout {
    fn vertex_input_bindings() -> Vec<vk::VertexInputBindingDescription>;

    fn vertex_input_attributes() -> Vec<vk::VertexInputAttributeDescription>;

    /// 整个 Buffer 的大小
    fn buffer_size(vertex_cnt: usize) -> usize;

    /// position 属性的 stride
    fn pos_stride() -> u32 {
        unimplemented!()
    }

    /// position 属性在 Buffer 中的偏移量
    fn pos_offset(_vertex_cnt: usize) -> vk::DeviceSize {
        unimplemented!()
    }
    /// normal 属性在 Buffer 中的偏移量
    fn normal_offset(_vertex_cnt: usize) -> vk::DeviceSize {
        unimplemented!()
    }
    /// tangent 属性在 Buffer 中的偏移量
    fn tangent_offset(_vertex_cnt: usize) -> vk::DeviceSize {
        unimplemented!()
    }
    /// uv 属性在 Buffer 中的偏移量
    fn uv_offset(_vertex_cnt: usize) -> vk::DeviceSize {
        unimplemented!()
    }
}
