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
    pub fn to_vk_type(&self) -> vk::WriteDescriptorSet<'_> {
        let mut descriptor_count = 0;
        let mut valid_count = 0;
        if !self.buffer_infos.is_empty() {
            descriptor_count = self.buffer_infos.len();
            valid_count += 1;
        }
        if !self.image_infos.is_empty() {
            descriptor_count = self.image_infos.len();
            valid_count += 1;
        }
        if !self.acceleration_structures.is_empty() {
            descriptor_count = 1;
            valid_count += 1;
        }

        assert_eq!(
            valid_count, 1,
            "Only one of buffer_infos, image_infos, or acceleration_structures should be set in GfxWriteDescriptorSet"
        );

        vk::WriteDescriptorSet {
            dst_set: self.dst_set,
            dst_binding: self.dst_binding,
            dst_array_element: self.dst_array_element,
            descriptor_count: descriptor_count as u32,
            descriptor_type: self.descriptor_type,
            // 选择 buffer ptr 还是 image ptr，是由 descriptor type 控制的
            p_buffer_info: self.buffer_infos.as_ptr(),
            p_image_info: self.image_infos.as_ptr(),
            ..Default::default()
        }
    }

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
        GfxWriteDescriptorSet {
            dst_set,
            dst_binding: item.binding,
            dst_array_element: start_array,
            buffer_infos: buffers,
            descriptor_type: item.descriptor_type,
            image_infos: vec![],
            acceleration_structures: vec![],
        }
    }

    /// 确保当前 descriptor 是 image
    fn write_image(
        &self,
        dst_set: vk::DescriptorSet,
        start_array: u32,
        images: Vec<vk::DescriptorImageInfo>,
    ) -> GfxWriteDescriptorSet {
        let item = self.get_binding();
        GfxWriteDescriptorSet {
            dst_set,
            dst_binding: item.binding,
            dst_array_element: start_array,
            descriptor_type: item.descriptor_type,
            buffer_infos: vec![],
            image_infos: images,
            acceleration_structures: vec![],
        }
    }

    /// 确保当前的 descriptor 是 tlas
    fn write_tals(
        &self,
        dst_set: vk::DescriptorSet,
        start_array: u32,
        tlas: Vec<vk::AccelerationStructureKHR>,
    ) -> GfxWriteDescriptorSet {
        let item = self.get_binding();
        GfxWriteDescriptorSet {
            dst_set,
            dst_binding: item.binding,
            dst_array_element: start_array,
            descriptor_type: item.descriptor_type,
            buffer_infos: vec![],
            image_infos: vec![],
            acceleration_structures: tlas,
        }
    }
}

impl GfxDescriptorCursor for DescriptorBindingItem {
    fn get_binding(&self) -> &DescriptorBindingItem {
        self
    }
}
