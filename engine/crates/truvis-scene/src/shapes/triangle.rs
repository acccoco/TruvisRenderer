use truvis_gfx::resources::special_buffers::index_buffer::GfxIndex32Buffer;
use truvis_gfx::resources::vertex_layout::soa_3d::VertexLayoutSoA3D;
use truvis_render_interface::geometry::RtGeometry;

/// 坐标系：RightHand, X-Right, Y-Up
///
/// 位于 XY 平面上的正立三角形，法线 +Z
///
/// 三角形绕序: CCW
///
/// ```text
///          y^
///           |
///           C (红色)
///          /|\
///         / | \
///        /  |  \
///       /   |   \
///      /    |    \
///     /     |     \
///    A------+------B---->x
/// (绿色)   |   (蓝色)
///           |
/// ```
pub struct TriangleSoA {}

impl TriangleSoA {
    // 3 个顶点：从 aos_pos_color 的 TRIANGLE_VERTEX_DATA 提取位置
    const POSITIONS: [glam::Vec3; 3] = [
        glam::vec3(-1.0, -1.0, 0.0), // A (左下, 绿色)
        glam::vec3(1.0, -1.0, 0.0),  // B (右下, 蓝色)
        glam::vec3(0.0, 1.0, 0.0),   // C (顶部, 红色)
    ];

    // 法线都指向 Z+ (朝向观察者)
    const NORMALS: [glam::Vec3; 3] = [
        glam::vec3(0.0, 0.0, 1.0),
        glam::vec3(0.0, 0.0, 1.0),
        glam::vec3(0.0, 0.0, 1.0),
    ];

    // UV 坐标：A(左下) B(右下) C(顶部)
    const UVS: [glam::Vec2; 3] = [
        glam::vec2(0.0, 1.0), // A
        glam::vec2(1.0, 1.0), // B
        glam::vec2(0.5, 0.0), // C
    ];

    // 切线指向 X+ (U 轴方向)
    const TANGENTS: [glam::Vec3; 3] = [
        glam::vec3(1.0, 0.0, 0.0),
        glam::vec3(1.0, 0.0, 0.0),
        glam::vec3(1.0, 0.0, 0.0),
    ];

    const INDICES: [u32; 3] = [0, 1, 2];

    pub fn create_mesh() -> RtGeometry {
        let vertex_buffer = VertexLayoutSoA3D::create_vertex_buffer(
            &Self::POSITIONS,
            &Self::NORMALS,
            &Self::TANGENTS,
            &Self::UVS,
            "triangle-vertex-buffer",
        );

        let index_buffer = GfxIndex32Buffer::new_device_local(Self::INDICES.len(), "triangle-index-buffer");
        index_buffer.transfer_data_sync(&Self::INDICES);

        RtGeometry {
            vertex_buffer,
            index_buffer,
        }
    }
}
