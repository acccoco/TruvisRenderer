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
/// vertex/index buffer 上传完成后，`AssetLoadService` 不再保存可直接查询的 mesh CPU 数据；
/// 自发光 light table 需要在 prepare 阶段按 active instance 重新展开 world-space
/// 三角形，因此这里把最小的 local-space position/uv/primitive id 跟随 GPU-ready mesh 缓存。
#[derive(Clone, Debug)]
pub(crate) struct RtTriangleMeta {
    pub(crate) positions: [glam::Vec3; 3],
    pub(crate) primitive_id: u32,
    /// 保留局部退化判定；小三角形可能经 instance 缩放后成为可采样的世界几何。
    pub(crate) local_area: f64,
}

impl RtTriangleMeta {
    /// 面积向量的长度是三角形面积，方向遵循顶点绕序。
    /// 先扩展再相减；有限 f32 顶点的叉积及长度平方可在 f64 中表示，避免 f32
    /// 中间平方把可表示的面积误算为零或无穷大。不恢复顶点存储/变换已丢失的精度。
    pub(crate) fn area_vector(positions: [glam::Vec3; 3]) -> glam::DVec3 {
        let [p0, p1, p2] = positions.map(|position| position.as_dvec3());
        0.5 * (p1 - p0).cross(p2 - p0)
    }
}

/// render-side mesh 的 GPU 几何资源。
///
/// 当前统一使用 `VertexLayoutSoA3D`，同一份 vertex/index buffer 同时服务光栅化 draw、
/// BLAS build 和 shader device address 读取。资源所有权由 `GpuMeshStore` 持有，
/// `RenderWorld` 只借用它生成 geometry table、TLAS instance 和 raster draw cache。
pub struct RtGeometry {
    /// SoA 顶点 buffer，按 position/normal/tangent/uv 四段提供 device address。
    pub vertex_buffer: GfxVertexBuffer<VertexLayoutSoA3D>,
    /// 32-bit index buffer；ray tracing 和 raster pass 使用同一索引类型。
    pub index_buffer: GfxIndex32Buffer,
    pub uv_set_count: u32,
    pub tangent_tex_coord: u32,
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
        u32::try_from(self.index_buffer.index_cnt()).expect("geometry index count exceeds u32")
    }
}

// 工具函数
impl RtGeometry {
    /// 构造 BLAS build 所需的 Vulkan geometry/range 描述。
    ///
    /// 输入 buffer 已经由 mesh 上传路径写入 device-local 内存；调用者需要在 copy 后建立
    /// `TRANSFER_WRITE -> ACCELERATION_STRUCTURE_BUILD` 的同步，再执行 BLAS build。
    pub fn get_blas_geometry_info(&self) -> GfxBlasInputInfo<'_> {
        let vertex_count = self.vertex_buffer.vertex_cnt();
        let max_vertex = u32::try_from(vertex_count.checked_sub(1).expect("BLAS geometry requires vertices"))
            .expect("BLAS maxVertex exceeds u32");
        let index_count = self.index_cnt();
        assert!(index_count > 0 && index_count % 3 == 0, "BLAS geometry requires complete triangles");
        // wrapper 可访问底层 buffer；投影前重新核对当前容量，不能只相信缓存 count。
        assert!(
            VertexLayoutSoA3D::buffer_size(vertex_count) as u64 <= self.vertex_buffer.size(),
            "BLAS vertex buffer is smaller than its layout"
        );
        let index_bytes = u64::from(index_count) * size_of::<u32>() as u64;
        assert!(index_bytes <= self.index_buffer.size(), "BLAS index buffer is smaller than its count");
        let vertex_address = self.vertex_buffer.pos_address();
        let index_address = self.index_buffer.device_address();
        assert!(vertex_address != 0 && vertex_address % 4 == 0, "BLAS vertex address must be nonzero and aligned");
        assert!(index_address != 0 && index_address % 4 == 0, "BLAS index address must be nonzero and aligned");
        vertex_address
            .checked_add((vertex_count as u64) * u64::from(VertexLayoutSoA3D::pos_stride()) - 1)
            .expect("BLAS vertex address range overflow");
        index_address.checked_add(index_bytes - 1).expect("BLAS index address range overflow");
        let geometry_triangle = vk::AccelerationStructureGeometryTrianglesDataKHR {
            vertex_format: vk::Format::R32G32B32_SFLOAT,
            vertex_data: vk::DeviceOrHostAddressConstKHR {
                device_address: vertex_address,
            },
            vertex_stride: VertexLayoutSoA3D::pos_stride() as vk::DeviceSize,
            // Vulkan maxVertex 是可访问顶点数量减一；先减再转换，保留完整 u32 索引范围。
            max_vertex,
            index_type: Self::index_type(),
            index_data: vk::DeviceOrHostAddressConstKHR {
                device_address: index_address,
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
                primitive_count: index_count / 3,
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
    /// 按 vertex/index owner 顺序释放；不捕获 panic 继续处理剩余资源。
    pub fn destroy_mut(&mut self, ctx: GfxResourceCtx<'_>, reason: DestroyReason) {
        self.vertex_buffer.destroy_mut(ctx, reason);
        self.index_buffer.destroy_mut(ctx, reason);
    }

    /// 消费 owner 并销毁几何资源。
    pub fn destroy(mut self, ctx: GfxResourceCtx<'_>, reason: DestroyReason) {
        self.destroy_mut(ctx, reason);
    }
}
