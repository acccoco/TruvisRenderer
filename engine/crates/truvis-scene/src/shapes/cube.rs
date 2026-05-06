use truvis_gfx::resources::special_buffers::index_buffer::GfxIndex32Buffer;
use truvis_gfx::resources::vertex_layout::soa_3d::VertexLayoutSoA3D;
use truvis_render_interface::geometry::RtGeometry;

/// 坐标系：右手系，X 向右，Y 向上
///
/// 三角形绕序: 逆时针 (CCW)
///
/// cube 尺寸：1
pub struct CubeSoA {}
impl CubeSoA {
    // 24 个顶点（每个面 4 个顶点，6 个面）
    const POSITIONS: [glam::Vec3; 24] = [
        // 顶面 (Y+)
        glam::vec3(0.5, 0.5, -0.5),  // 0: TOP_A
        glam::vec3(-0.5, 0.5, -0.5), // 1: TOP_B
        glam::vec3(-0.5, 0.5, 0.5),  // 2: TOP_C
        glam::vec3(0.5, 0.5, 0.5),   // 3: TOP_D
        // 底面 (Y-)
        glam::vec3(0.5, -0.5, -0.5),  // 4: BOTTOM_A
        glam::vec3(-0.5, -0.5, -0.5), // 5: BOTTOM_B
        glam::vec3(-0.5, -0.5, 0.5),  // 6: BOTTOM_C
        glam::vec3(0.5, -0.5, 0.5),   // 7: BOTTOM_D
        // 近端面 (Z+)
        glam::vec3(0.5, 0.5, 0.5),   // 8: NEAR_A
        glam::vec3(-0.5, 0.5, 0.5),  // 9: NEAR_B
        glam::vec3(-0.5, -0.5, 0.5), // 10: NEAR_C
        glam::vec3(0.5, -0.5, 0.5),  // 11: NEAR_D
        // 远端面 (Z-)
        glam::vec3(0.5, 0.5, -0.5),   // 12: FAR_A
        glam::vec3(-0.5, 0.5, -0.5),  // 13: FAR_B
        glam::vec3(-0.5, -0.5, -0.5), // 14: FAR_C
        glam::vec3(0.5, -0.5, -0.5),  // 15: FAR_D
        // 左侧面 (X-)
        glam::vec3(-0.5, 0.5, 0.5),   // 16: LEFT_A
        glam::vec3(-0.5, 0.5, -0.5),  // 17: LEFT_B
        glam::vec3(-0.5, -0.5, -0.5), // 18: LEFT_C
        glam::vec3(-0.5, -0.5, 0.5),  // 19: LEFT_D
        // 右侧面 (X+)
        glam::vec3(0.5, 0.5, 0.5),   // 20: RIGHT_A
        glam::vec3(0.5, 0.5, -0.5),  // 21: RIGHT_B
        glam::vec3(0.5, -0.5, -0.5), // 22: RIGHT_C
        glam::vec3(0.5, -0.5, 0.5),  // 23: RIGHT_D
    ];

    const NORMALS: [glam::Vec3; 24] = [
        // 顶面 (Y+)
        glam::vec3(0.0, 1.0, 0.0),
        glam::vec3(0.0, 1.0, 0.0),
        glam::vec3(0.0, 1.0, 0.0),
        glam::vec3(0.0, 1.0, 0.0),
        // 底面 (Y-)
        glam::vec3(0.0, -1.0, 0.0),
        glam::vec3(0.0, -1.0, 0.0),
        glam::vec3(0.0, -1.0, 0.0),
        glam::vec3(0.0, -1.0, 0.0),
        // 近端面 (Z+)
        glam::vec3(0.0, 0.0, 1.0),
        glam::vec3(0.0, 0.0, 1.0),
        glam::vec3(0.0, 0.0, 1.0),
        glam::vec3(0.0, 0.0, 1.0),
        // 远端面 (Z-)
        glam::vec3(0.0, 0.0, -1.0),
        glam::vec3(0.0, 0.0, -1.0),
        glam::vec3(0.0, 0.0, -1.0),
        glam::vec3(0.0, 0.0, -1.0),
        // 左侧面 (X-)
        glam::vec3(-1.0, 0.0, 0.0),
        glam::vec3(-1.0, 0.0, 0.0),
        glam::vec3(-1.0, 0.0, 0.0),
        glam::vec3(-1.0, 0.0, 0.0),
        // 右侧面 (X+)
        glam::vec3(1.0, 0.0, 0.0),
        glam::vec3(1.0, 0.0, 0.0),
        glam::vec3(1.0, 0.0, 0.0),
        glam::vec3(1.0, 0.0, 0.0),
    ];

    const UVS: [glam::Vec2; 24] = [
        // 顶面 (Y+)
        glam::vec2(1.0, 0.0),
        glam::vec2(0.0, 0.0),
        glam::vec2(0.0, 1.0),
        glam::vec2(1.0, 1.0),
        // 底面 (Y-)
        glam::vec2(1.0, 0.0),
        glam::vec2(0.0, 0.0),
        glam::vec2(0.0, 1.0),
        glam::vec2(1.0, 1.0),
        // 近端面 (Z+)
        glam::vec2(1.0, 0.0),
        glam::vec2(0.0, 0.0),
        glam::vec2(0.0, 1.0),
        glam::vec2(1.0, 1.0),
        // 远端面 (Z-)
        glam::vec2(1.0, 0.0),
        glam::vec2(0.0, 0.0),
        glam::vec2(0.0, 1.0),
        glam::vec2(1.0, 1.0),
        // 左侧面 (X-)
        glam::vec2(1.0, 0.0),
        glam::vec2(0.0, 0.0),
        glam::vec2(0.0, 1.0),
        glam::vec2(1.0, 1.0),
        // 右侧面 (X+)
        glam::vec2(1.0, 0.0),
        glam::vec2(0.0, 0.0),
        glam::vec2(0.0, 1.0),
        glam::vec2(1.0, 1.0),
    ];

    // 切线向量指向 U 轴正方向
    // 对于每个面，切线与法线、副切线构成右手坐标系
    const TANGENTS: [glam::Vec3; 24] = [
        // 顶面 (Y+，法线 Y+)：tangent 指向 X+ (U 轴方向)
        glam::vec3(1.0, 0.0, 0.0),
        glam::vec3(1.0, 0.0, 0.0),
        glam::vec3(1.0, 0.0, 0.0),
        glam::vec3(1.0, 0.0, 0.0),
        // 底面 (Y-，法线 Y-)：tangent 指向 X+ (U 轴方向)
        glam::vec3(1.0, 0.0, 0.0),
        glam::vec3(1.0, 0.0, 0.0),
        glam::vec3(1.0, 0.0, 0.0),
        glam::vec3(1.0, 0.0, 0.0),
        // 近端面 (Z+，法线 Z+)：tangent 指向 X+ (U 轴方向)
        glam::vec3(1.0, 0.0, 0.0),
        glam::vec3(1.0, 0.0, 0.0),
        glam::vec3(1.0, 0.0, 0.0),
        glam::vec3(1.0, 0.0, 0.0),
        // 远端面 (Z-，法线 Z-)：tangent 指向 X+ (U 轴方向)
        glam::vec3(1.0, 0.0, 0.0),
        glam::vec3(1.0, 0.0, 0.0),
        glam::vec3(1.0, 0.0, 0.0),
        glam::vec3(1.0, 0.0, 0.0),
        // 左侧面 (X-，法线 X-)：tangent 指向 Z+ (U 轴方向)
        glam::vec3(0.0, 0.0, 1.0),
        glam::vec3(0.0, 0.0, 1.0),
        glam::vec3(0.0, 0.0, 1.0),
        glam::vec3(0.0, 0.0, 1.0),
        // 右侧面 (X+，法线 X+)：tangent 指向 Z+ (U 轴方向)
        glam::vec3(0.0, 0.0, 1.0),
        glam::vec3(0.0, 0.0, 1.0),
        glam::vec3(0.0, 0.0, 1.0),
        glam::vec3(0.0, 0.0, 1.0),
    ];

    const INDICES: [u32; 36] = [
        0, 1, 2, 0, 2, 3, // 顶面
        4, 6, 5, 4, 7, 6, // 底面
        8, 9, 10, 8, 10, 11, // 近端面
        12, 14, 13, 12, 15, 14, // 远端面
        16, 17, 18, 16, 18, 19, // 左侧面
        20, 22, 21, 20, 23, 22, // 右侧面
    ];

    pub fn create_mesh() -> RtGeometry {
        let vertex_buffer = VertexLayoutSoA3D::create_vertex_buffer(
            &Self::POSITIONS,
            &Self::NORMALS,
            &Self::TANGENTS,
            &Self::UVS,
            "cube-vertex-buffer",
        );

        let index_buffer = GfxIndex32Buffer::new_device_local(Self::INDICES.len(), "cube-index-buffer");
        index_buffer.transfer_data_sync(&Self::INDICES);

        RtGeometry {
            vertex_buffer,
            index_buffer,
        }
    }
}
