use ash::vk;
use itertools::Itertools;

use crate::gfx::Gfx;
use crate::resources::layout::GfxIndexType;
use crate::resources::special_buffers::index_buffer::GfxIndexBuffer;
use crate::utilities::descriptor_cursor::GfxWriteDescriptorSet;
use crate::{
    basic::color::LabelColor,
    commands::{
        barrier::{GfxBufferBarrier, GfxImageBarrier},
        command_pool::GfxCommandPool,
    },
    foundation::debug_messenger::DebugType,
    pipelines::rendering_info::GfxRenderingInfo,
    query::query_pool::GfxQueryPool,
    resources::buffer::GfxBuffer,
};

/// 命令缓冲封装
///
/// 封装 Vulkan CommandBuffer，提供类型安全的命令录制接口。
/// 支持图形、计算、光线追踪、屏障、调试标签等功能。
///
/// # 使用示例
/// ```ignore
/// let cmd = CommandBuffer::new(&pool, "my-pass");
/// cmd.begin(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT, "my-pass");
/// cmd.cmd_bind_pipeline(vk::PipelineBindPoint::GRAPHICS, pipeline);
/// // 绘制命令...
/// cmd.end();
/// ```
#[derive(Clone)]
pub struct GfxCommandBuffer {
    vk_handle: vk::CommandBuffer,
    _command_pool_handle: vk::CommandPool,

    _name: String,
}
// 创建与初始化
impl GfxCommandBuffer {
    pub fn new(command_pool: &GfxCommandPool, debug_name: &str) -> Self {
        let info = vk::CommandBufferAllocateInfo::default()
            .command_pool(command_pool.handle())
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(1);

        let command_buffer = unsafe { Gfx::get().gfx_device().allocate_command_buffers(&info).unwrap()[0] };
        let cmd_buffer = GfxCommandBuffer {
            vk_handle: command_buffer,
            _command_pool_handle: command_pool.handle(),

            _name: debug_name.to_string(),
        };
        Gfx::get().gfx_device().set_debug_name(&cmd_buffer, debug_name);
        cmd_buffer
    }
}
// Basic 命令
impl GfxCommandBuffer {
    /// 开始录制 command
    ///
    /// 自动设置 debug label
    #[inline]
    pub fn begin(&self, usage_flag: vk::CommandBufferUsageFlags, debug_label_name: &str) {
        unsafe {
            Gfx::get()
                .gfx_device()
                .begin_command_buffer(self.vk_handle, &vk::CommandBufferBeginInfo::default().flags(usage_flag))
                .unwrap();
        }
        self.begin_label(debug_label_name, LabelColor::COLOR_CMD);
    }

    /// 结束录制 command
    ///
    /// 结束 debug label
    #[inline]
    pub fn end(&self) {
        self.end_label();
        unsafe { Gfx::get().gfx_device().end_command_buffer(self.vk_handle).unwrap() }
    }
}
// 访问器
impl GfxCommandBuffer {
    /// 访问器
    #[inline]
    pub fn vk_handle(&self) -> vk::CommandBuffer {
        self.vk_handle
    }
}
// 数据传输类型
impl GfxCommandBuffer {
    /// - 命令类型：action
    /// - 支持的 queue：transfer，graphics，compute
    #[inline]
    pub fn cmd_copy_buffer(&self, src: &GfxBuffer, dst: &GfxBuffer, regions: &[vk::BufferCopy]) {
        unsafe {
            Gfx::get().gfx_device().cmd_copy_buffer(self.vk_handle, src.vk_buffer(), dst.vk_buffer(), regions);
        }
    }

    /// - 命令类型：action
    /// - 支持的 queue：transfer，graphics，compute
    #[inline]
    pub fn cmd_copy_buffer_to_image(&self, copy_info: &vk::CopyBufferToImageInfo2) {
        unsafe { Gfx::get().gfx_device().cmd_copy_buffer_to_image2(self.vk_handle, copy_info) }
    }

    /// 将 data 传输到 buffer 中，大小限制：65536Bytes=64KB
    ///
    /// 首先将 data 复制到 cmd buffer 中，然后再 transfer 到指定 buffer
    /// 中，这是一个 transfer op
    ///
    /// 需要在 render pass 之外进行，注意同步
    ///
    ///
    /// - 命令类型：action
    /// - 支持的 queue 类型：transfer, graphics, compute
    #[inline]
    pub fn cmd_update_buffer(&self, buffer: vk::Buffer, offset: vk::DeviceSize, data: &[u8]) {
        unsafe { Gfx::get().gfx_device().cmd_update_buffer(self.vk_handle, buffer, offset, data) }
    }

    /// 视为 transfer op
    ///
    /// 需要进行同步
    pub fn cmd_fill_buffer(&self, dst_buffer: vk::Buffer, dst_offset: vk::DeviceSize, size: vk::DeviceSize, data: u32) {
        unsafe {
            Gfx::get().gfx_device().cmd_fill_buffer(self.vk_handle, dst_buffer, dst_offset, size, data);
        }
    }

    /// - 命令类型：state
    /// - 支持的 queue: graphics, compute
    #[inline]
    pub fn cmd_push_constants(
        &self,
        pipeline_layout: vk::PipelineLayout,
        stage: vk::ShaderStageFlags,
        offset: u32,
        data: &[u8],
    ) {
        unsafe {
            Gfx::get().gfx_device().cmd_push_constants(self.vk_handle, pipeline_layout, stage, offset, data);
        }
    }
}
// 绘制类型的命令
impl GfxCommandBuffer {
    /// - 命令类型：action, state
    /// - 支持的 queue 类型：graphics
    #[inline]
    pub fn cmd_begin_rendering(&self, render_info: &vk::RenderingInfo) {
        unsafe {
            Gfx::get().gfx_device().dynamic_rendering.cmd_begin_rendering(self.vk_handle, render_info);
        }
    }

    pub fn cmd_begin_rendering2(&self, rendering_info: &GfxRenderingInfo) {
        let rendering_info = rendering_info.rendering_info();
        unsafe {
            Gfx::get().gfx_device().dynamic_rendering.cmd_begin_rendering(self.vk_handle, &rendering_info);
        }
    }

    /// - 命令类型：action, state
    /// - 支持的 queue 类型：graphics
    #[inline]
    pub fn end_rendering(&self) {
        unsafe {
            Gfx::get().gfx_device().dynamic_rendering.cmd_end_rendering(self.vk_handle);
        }
    }

    /// - 命令类型：action
    /// - 支持的 queue 类型：graphics
    #[inline]
    pub fn draw_indexed(
        &self,
        index_cnt: u32,
        first_index: u32,
        instance_cnt: u32,
        first_instance: u32,
        vertex_offset: i32,
    ) {
        unsafe {
            Gfx::get().gfx_device().cmd_draw_indexed(
                self.vk_handle,
                index_cnt,
                instance_cnt,
                first_index,
                vertex_offset,
                first_instance,
            );
        }
    }

    /// - 命令类型：action
    /// - 支持的 queue 类型：graphics
    ///
    /// 不使用 index buffer 的绘制
    #[inline]
    pub fn cmd_draw(&self, vertex_count: u32, instance_count: u32, first_vertex: u32, first_instance: u32) {
        unsafe {
            Gfx::get().gfx_device().cmd_draw(
                self.vk_handle,
                vertex_count,
                instance_count,
                first_vertex,
                first_instance,
            );
        }
    }

    /// - 命令类型：state
    /// - 支持的 queue 类型：graphics, compute
    #[inline]
    pub fn bind_descriptor_sets(
        &self,
        bind_point: vk::PipelineBindPoint,
        pipeline_layout: vk::PipelineLayout,
        first_set: u32,
        descriptor_sets: &[vk::DescriptorSet],
        dynamic_offsets: Option<&[u32]>,
    ) {
        unsafe {
            Gfx::get().gfx_device().cmd_bind_descriptor_sets(
                self.vk_handle,
                bind_point,
                pipeline_layout,
                first_set,
                descriptor_sets,
                dynamic_offsets.unwrap_or(&[]),
            );
        }
    }

    #[inline]
    pub fn push_descriptor_set(
        &self,
        bind_point: vk::PipelineBindPoint,
        pipeline_layout: vk::PipelineLayout,
        set: u32,
        writes: &[GfxWriteDescriptorSet],
    ) {
        GfxWriteDescriptorSet::with_writes(writes, |writes| unsafe {
            Gfx::get().gfx_device().push_descriptor.cmd_push_descriptor_set(
                self.vk_handle,
                bind_point,
                pipeline_layout,
                set,
                writes,
            );
        })
    }

    /// - 命令类型：state
    /// - 支持的 queue 类型：graphics, compute
    #[inline]
    pub fn cmd_bind_pipeline(&self, bind_point: vk::PipelineBindPoint, pipeline: vk::Pipeline) {
        unsafe {
            Gfx::get().gfx_device().cmd_bind_pipeline(self.vk_handle, bind_point, pipeline);
        }
    }

    /// buffers 每个 vertex buffer 以及 offset
    /// - 命令类型：state
    /// - 支持的 queue 类型：graphics
    #[inline]
    pub fn cmd_bind_vertex_buffers(&self, first_bind: u32, buffers: &[vk::Buffer], offsets: &[vk::DeviceSize]) {
        unsafe {
            Gfx::get().gfx_device().cmd_bind_vertex_buffers(self.vk_handle, first_bind, buffers, offsets);
        }
    }

    /// - 命令类型：state
    /// - 支持的 queue 类型：graphics
    #[inline]
    pub fn cmd_bind_index_buffer<T: GfxIndexType>(&self, buffer: &GfxIndexBuffer<T>, offset: vk::DeviceSize) {
        unsafe {
            Gfx::get().gfx_device().cmd_bind_index_buffer(self.vk_handle, buffer.vk_buffer(), offset, T::VK_INDEX_TYPE);
        }
    }

    /// - 命令类型：state
    /// - 支持的 queue 类型：graphics
    #[inline]
    pub fn cmd_set_viewport(&self, first_viewport: u32, viewports: &[vk::Viewport]) {
        unsafe {
            Gfx::get().gfx_device().cmd_set_viewport(self.vk_handle, first_viewport, viewports);
        }
    }

    /// - 命令类型：state
    /// - 支持的 queue 类型：graphics
    #[inline]
    pub fn cmd_set_scissor(&self, first_scissor: u32, scissors: &[vk::Rect2D]) {
        unsafe {
            Gfx::get().gfx_device().cmd_set_scissor(self.vk_handle, first_scissor, scissors);
        }
    }
}
// 光追相关
impl GfxCommandBuffer {
    /// - 命令类型：action
    /// - 支持的 queue 类型：compute
    #[inline]
    pub fn cmd_copy_acceleration_structure(&self, copy_info: &vk::CopyAccelerationStructureInfoKHR) {
        unsafe {
            Gfx::get().gfx_device().acceleration_structure.cmd_copy_acceleration_structure(self.vk_handle, copy_info);
        }
    }

    /// - 命令类型：action
    /// - 支持的 queue 类型：compute
    #[inline]
    pub fn build_acceleration_structure(
        &self,
        geometry: &vk::AccelerationStructureBuildGeometryInfoKHR,
        ranges: &[vk::AccelerationStructureBuildRangeInfoKHR],
    ) {
        unsafe {
            // 该函数可以一次构建多个 AccelerationStructure，这里只构建了 1 个
            Gfx::get().gfx_device().acceleration_structure.cmd_build_acceleration_structures(
                self.vk_handle,
                std::slice::from_ref(geometry),
                &[ranges],
            )
        }
    }

    /// 这里涉及到对加速结构的 read，需要同步
    /// - 命令类型：action
    /// - 支持的 queue 类型：compute
    #[inline]
    pub fn write_acceleration_structure_properties(
        &self,
        query_pool: &mut GfxQueryPool,
        first_query: u32,
        acceleration_structures: &[vk::AccelerationStructureKHR],
    ) {
        unsafe {
            Gfx::get().gfx_device().acceleration_structure.cmd_write_acceleration_structures_properties(
                self.vk_handle,
                acceleration_structures,
                query_pool.query_type(),
                query_pool.handle(),
                first_query,
            )
        }
    }

    /// 光追的入口
    /// - 命令类型：action
    /// - 支持的 queue 类型：compute
    #[inline]
    pub fn trace_rays(
        &self,
        raygen_table: &vk::StridedDeviceAddressRegionKHR,
        miss_table: &vk::StridedDeviceAddressRegionKHR,
        hit_table: &vk::StridedDeviceAddressRegionKHR,
        callable_table: &vk::StridedDeviceAddressRegionKHR,
        thread_size: [u32; 3],
    ) {
        unsafe {
            Gfx::get().gfx_device().ray_tracing_pipeline.cmd_trace_rays(
                self.vk_handle,
                raygen_table,
                miss_table,
                hit_table,
                callable_table,
                thread_size[0],
                thread_size[1],
                thread_size[2],
            );
        }
    }
}
// 计算着色器相关命令
impl GfxCommandBuffer {
    #[inline]
    pub fn cmd_dispatch(&self, group_cnt: glam::UVec3) {
        unsafe {
            Gfx::get().gfx_device().cmd_dispatch(self.vk_handle, group_cnt.x, group_cnt.y, group_cnt.z);
        }
    }
}
// 同步相关命令
impl GfxCommandBuffer {
    /// - 命令类型：synchronize
    /// - 支持的 queue 类型：graphics, compute, transfer
    #[inline]
    pub fn memory_barrier(&self, barriers: &[vk::MemoryBarrier2]) {
        let dependency_info = vk::DependencyInfo::default().memory_barriers(barriers);
        unsafe {
            Gfx::get().gfx_device().cmd_pipeline_barrier2(self.vk_handle, &dependency_info);
        }
    }

    /// - 命令类型：synchronize
    /// - 支持的 queue 类型：graphics, compute, transfer
    #[inline]
    pub fn image_memory_barrier(&self, dependency_flags: vk::DependencyFlags, barriers: &[GfxImageBarrier]) {
        let barriers = barriers.iter().map(|b| *b.inner()).collect_vec();
        let dependency_info =
            vk::DependencyInfo::default().image_memory_barriers(&barriers).dependency_flags(dependency_flags);
        unsafe {
            Gfx::get().gfx_device().cmd_pipeline_barrier2(self.vk_handle, &dependency_info);
        }
    }

    /// - 命令类型：synchronize
    /// - 支持的 queue 类型：graphics, compute, transfer
    #[inline]
    pub fn buffer_memory_barrier(&self, dependency_flags: vk::DependencyFlags, barriers: &[GfxBufferBarrier]) {
        let barriers = barriers.iter().map(|b| *b.inner()).collect_vec();
        let dependency_info =
            vk::DependencyInfo::default().buffer_memory_barriers(&barriers).dependency_flags(dependency_flags);
        unsafe {
            Gfx::get().gfx_device().cmd_pipeline_barrier2(self.vk_handle, &dependency_info);
        }
    }
}
// debug 相关命令
impl GfxCommandBuffer {
    /// - 命令类型：state, action
    /// - 支持的 queue 类型：graphics, compute
    #[inline]
    pub fn begin_label(&self, label_name: &str, label_color: glam::Vec4) {
        let name = std::ffi::CString::new(label_name).unwrap();
        unsafe {
            Gfx::get().gfx_device().debug_utils.cmd_begin_debug_utils_label(
                self.vk_handle,
                &vk::DebugUtilsLabelEXT::default().label_name(name.as_c_str()).color(label_color.into()),
            );
        }
    }

    /// - 命令类型：state, action
    /// - 支持的 queue 类型：graphics, compute
    #[inline]
    pub fn end_label(&self) {
        unsafe {
            Gfx::get().gfx_device().debug_utils.cmd_end_debug_utils_label(self.vk_handle);
        }
    }

    /// - 命令类型：action
    /// - 支持的 queue 类型：graphics, compute
    #[inline]
    pub fn insert_label(&self, label_name: &str, label_color: glam::Vec4) {
        let name = std::ffi::CString::new(label_name).unwrap();
        unsafe {
            Gfx::get().gfx_device().debug_utils.cmd_insert_debug_utils_label(
                self.vk_handle,
                &vk::DebugUtilsLabelEXT::default().label_name(name.as_c_str()).color(label_color.into()),
            );
        }
    }
}
impl DebugType for GfxCommandBuffer {
    fn debug_type_name() -> &'static str {
        "GfxCommandBuffer"
    }

    fn vk_handle(&self) -> impl vk::Handle {
        self.vk_handle
    }
}
