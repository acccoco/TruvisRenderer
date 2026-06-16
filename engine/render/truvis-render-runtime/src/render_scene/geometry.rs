use ash::vk;

use truvis_gfx::gfx::GfxResourceCtx;
use truvis_gfx::raytracing::acceleration::GfxBlasInputInfo;
use truvis_gfx::resources::layout::GfxVertexLayout;
use truvis_gfx::resources::lifecycle::DestroyReason;
use truvis_gfx::resources::special_buffers::index_buffer::GfxIndex32Buffer;
use truvis_gfx::resources::special_buffers::vertex_buffer::GfxVertexBuffer;
use truvis_gfx::resources::vertex_layout::soa_3d::VertexLayoutSoA3D;

/// render-side 保留的 CPU 三角形元数据。
///
/// vertex/index buffer 上传完成后，`AssetHub` 不再保存可直接查询的 mesh CPU 数据；
/// 自发光 light table 需要在 prepare 阶段按 active instance 重新展开 world-space
/// 三角形，因此这里把最小的 local-space position/uv/primitive id 跟随 GPU-ready mesh 缓存。
#[derive(Clone, Copy, Debug)]
pub(crate) struct RtTriangleMeta {
    pub(crate) positions: [glam::Vec3; 3],
    pub(crate) uvs: [glam::Vec2; 3],
    pub(crate) primitive_id: u32,
    pub(crate) local_area: f32,
}

/// render-side mesh 的 GPU 几何资源。
///
/// 当前统一使用 `VertexLayoutSoA3D`，同一份 vertex/index buffer 同时服务光栅化 draw、
/// BLAS build 和 shader device address 读取。资源所有权由 `AssetMeshManager` 持有，
/// `GpuScene` 只借用它生成 geometry table、TLAS instance 和 raster draw cache。
pub struct RtGeometry {
    /// SoA 顶点 buffer，按 position/normal/tangent/uv 四段提供 device address。
    pub vertex_buffer: GfxVertexBuffer<VertexLayoutSoA3D>,
    /// 32-bit index buffer；ray tracing 和 raster pass 使用同一索引类型。
    pub index_buffer: GfxIndex32Buffer,
}

// 访问器
impl RtGeometry {
    /// 当前 geometry 统一使用 32-bit index，必须与 mesh manager 创建的 index buffer 保持一致。
    #[inline]
    pub fn index_type() -> vk::IndexType {
        vk::IndexType::UINT32
    }

    /// index 数量，供 raster draw 和 BLAS primitive_count 计算使用。
    #[inline]
    pub fn index_cnt(&self) -> u32 {
        self.index_buffer.index_cnt() as u32
    }
}

// 工具函数
impl RtGeometry {
    /// 构造 BLAS build 所需的 Vulkan geometry/range 描述。
    ///
    /// 输入 buffer 已经由 mesh 上传路径写入 device-local 内存；调用者需要在 copy 后建立
    /// `TRANSFER_WRITE -> ACCELERATION_STRUCTURE_BUILD` 的同步，再执行 BLAS build。
    pub fn get_blas_geometry_info(&self) -> GfxBlasInputInfo<'_> {
        let geometry_triangle = vk::AccelerationStructureGeometryTrianglesDataKHR {
            vertex_format: vk::Format::R32G32B32_SFLOAT,
            vertex_data: vk::DeviceOrHostAddressConstKHR {
                device_address: self.vertex_buffer.pos_address(),
            },
            vertex_stride: VertexLayoutSoA3D::pos_stride() as vk::DeviceSize,
            // spec 上说应该是 vertex cnt - 1，应该是用作 index
            max_vertex: self.vertex_buffer.vertex_cnt() as u32 - 1,
            index_type: Self::index_type(),
            index_data: vk::DeviceOrHostAddressConstKHR {
                device_address: self.index_buffer.device_address(),
            },

            // 并不需要为每个 geometry 设置变换数据
            transform_data: vk::DeviceOrHostAddressConstKHR::default(),

            ..Default::default()
        };

        GfxBlasInputInfo {
            geometry: vk::AccelerationStructureGeometryKHR::default()
                .geometry_type(vk::GeometryTypeKHR::TRIANGLES)
                // OPAQUE 表示永远不会调用 anyhit shader
                // NO_DUPLICATE 表示 primitive 只会被 any hit shader 命中一次
                .flags(vk::GeometryFlagsKHR::NO_DUPLICATE_ANY_HIT_INVOCATION)
                .geometry(vk::AccelerationStructureGeometryDataKHR {
                    triangles: geometry_triangle,
                }),
            range: vk::AccelerationStructureBuildRangeInfoKHR {
                primitive_count: self.index_cnt() / 3,
                primitive_offset: 0,
                first_vertex: 0,
                // 如果上方的 geometry data 中 的 transform_data 有数据，则该 offset 用于指定
                // transform 的 bytes offset
                transform_offset: 0,
            },
        }
    }
}

impl RtGeometry {
    /// 显式销毁几何资源；调用者负责确保没有在飞命令继续读取这些 buffer。
    pub fn destroy_mut(&mut self, ctx: GfxResourceCtx<'_>, reason: DestroyReason) {
        self.vertex_buffer.destroy_mut(ctx, reason);
        self.index_buffer.destroy_mut(ctx, reason);
    }

    /// 消费 owner 并销毁几何资源。
    pub fn destroy(mut self, ctx: GfxResourceCtx<'_>, reason: DestroyReason) {
        self.destroy_mut(ctx, reason);
    }
}
