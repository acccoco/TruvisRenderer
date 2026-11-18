use ash::vk;
use itertools::Itertools;

use truvis_descriptor_layout_trait::DescriptorBindingItem;

pub struct GfxWriteDescriptorSet {
    pub dst_set: vk::DescriptorSet,
    pub dst_binding: u32,
    pub dst_array_element: u32,
    pub descriptor_type: vk::DescriptorType,

    pub buffer_infos: Vec<vk::DescriptorBufferInfo>,
    pub image_infos: Vec<vk::DescriptorImageInfo>,
    pub acceleration_structures: Vec<vk::AccelerationStructureKHR>,
}
impl GfxWriteDescriptorSet {
    /// descriptor type 决定 Vulkan 读取哪个数组，不能仅根据任一非空数组推导写入数量。
    fn descriptor_count(&self) -> u32 {
        let valid_count = usize::from(!self.buffer_infos.is_empty())
            + usize::from(!self.image_infos.is_empty())
            + usize::from(!self.acceleration_structures.is_empty());
        assert_eq!(
            valid_count, 1,
            "Only one of buffer_infos, image_infos, or acceleration_structures should be set in GfxWriteDescriptorSet"
        );
        let count = match self.descriptor_type {
            vk::DescriptorType::UNIFORM_BUFFER
            | vk::DescriptorType::STORAGE_BUFFER
            | vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC
            | vk::DescriptorType::STORAGE_BUFFER_DYNAMIC => self.buffer_infos.len(),
            vk::DescriptorType::SAMPLER
            | vk::DescriptorType::COMBINED_IMAGE_SAMPLER
            | vk::DescriptorType::SAMPLED_IMAGE
            | vk::DescriptorType::STORAGE_IMAGE
            | vk::DescriptorType::INPUT_ATTACHMENT => self.image_infos.len(),
            vk::DescriptorType::ACCELERATION_STRUCTURE_KHR => self.acceleration_structures.len(),
            other => panic!("GfxWriteDescriptorSet 不支持 descriptor type: {other:?}"),
        };
        assert!(count > 0, "descriptor type 与写入 payload 不匹配");
        u32::try_from(count).expect("descriptor 写入数量超过 u32")
    }

    /// cursor 只写自身 binding，不使用 Vulkan 的跨连续 binding 更新语义。
    fn validate_binding_range(&self, binding: &DescriptorBindingItem) {
        let count = self.descriptor_count();
        assert!(
            self.dst_array_element.checked_add(count).is_some_and(|end| end <= binding.count),
            "descriptor 写入超出 binding {} 的容量 {}",
            binding.name,
            binding.count
        );
    }

    /// 加速结构的 pNext 由 with_writes 在回调期间补齐；此基础投影不单独交给 Vulkan。
    fn to_vk_type(&self) -> vk::WriteDescriptorSet<'_> {
        vk::WriteDescriptorSet {
            dst_set: self.dst_set,
            dst_binding: self.dst_binding,
            dst_array_element: self.dst_array_element,
            descriptor_count: self.descriptor_count(),
            descriptor_type: self.descriptor_type,
            // 选择 buffer ptr 还是 image ptr，是由 descriptor type 控制的
            p_buffer_info: self.buffer_infos.as_ptr(),
            p_image_info: self.image_infos.as_ptr(),
            ..Default::default()
        }
    }

    /// payload 与扩展数组在同步回调返回前有效，调用方不得缓存其中的 CPU 指针。
    pub fn with_writes(writes: &[Self], cbk: impl Fn(&[vk::WriteDescriptorSet])) {
        let mut write_accs = writes
            .iter()
            .map(|w| {
                vk::WriteDescriptorSetAccelerationStructureKHR::default()
                    .acceleration_structures(&w.acceleration_structures)
            })
            .collect_vec();
        let writes = writes.iter().map(|w| w.to_vk_type()).collect_vec();
        let writes = writes
            .into_iter()
            .zip(write_accs.iter_mut())
            .map(|(w, write_acc)| if write_acc.acceleration_structure_count > 0 { w.push_next(write_acc) } else { w })
            .collect_vec();
        cbk(&writes);
    }
}

/// 用于通过 DescriptorBinding Item 来操作对应 descriptor set 的对应 binding
pub trait GfxDescriptorCursor {
    fn get_binding(&self) -> &DescriptorBindingItem;

    /// 确保当前 descriptor 是 buffer
    fn write_buffer(
        &self,
        dst_set: vk::DescriptorSet,
        start_array: u32,
        buffers: Vec<vk::DescriptorBufferInfo>,
    ) -> GfxWriteDescriptorSet {
        let item = self.get_binding();
        let write = GfxWriteDescriptorSet {
            dst_set,
            dst_binding: item.binding,
            dst_array_element: start_array,
            buffer_infos: buffers,
            descriptor_type: item.descriptor_type,
            image_infos: vec![],
            acceleration_structures: vec![],
        };
        write.validate_binding_range(item);
        write
    }

    /// 确保当前 descriptor 是 image
    fn write_image(
        &self,
        dst_set: vk::DescriptorSet,
        start_array: u32,
        images: Vec<vk::DescriptorImageInfo>,
    ) -> GfxWriteDescriptorSet {
        let item = self.get_binding();
        let write = GfxWriteDescriptorSet {
            dst_set,
            dst_binding: item.binding,
            dst_array_element: start_array,
            descriptor_type: item.descriptor_type,
            buffer_infos: vec![],
            image_infos: images,
            acceleration_structures: vec![],
        };
        write.validate_binding_range(item);
        write
    }

    /// 确保当前的 descriptor 是 tlas
    fn write_tals(
        &self,
        dst_set: vk::DescriptorSet,
        start_array: u32,
        tlas: Vec<vk::AccelerationStructureKHR>,
    ) -> GfxWriteDescriptorSet {
        let item = self.get_binding();
        let write = GfxWriteDescriptorSet {
            dst_set,
            dst_binding: item.binding,
            dst_array_element: start_array,
            descriptor_type: item.descriptor_type,
            buffer_infos: vec![],
            image_infos: vec![],
            acceleration_structures: tlas,
        };
        write.validate_binding_range(item);
        write
    }
}

impl GfxDescriptorCursor for DescriptorBindingItem {
    fn get_binding(&self) -> &DescriptorBindingItem {
        self
    }
}
