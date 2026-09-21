use truvis_asset::handle::{MeshData, SubmeshData};

/// 内置程序化 mesh 类型。
///
/// 这些数据只描述 CPU 侧顶点属性和索引，不创建 GPU buffer 或 BLAS。调用方应通过
/// `GameWorld::register_mesh` 注册后进入标准 `GpuMeshStore` 路径。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ProceduralMeshKind {
    Triangle,
    Rect,
    Floor,
    Cube,
    UvSphere,
}

impl ProceduralMeshKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Triangle => "procedural-triangle",
            Self::Rect => "procedural-rect",
            Self::Floor => "procedural-floor",
            Self::Cube => "procedural-cube",
            Self::UvSphere => "procedural-uv-sphere",
        }
    }

    pub fn mesh_data(self) -> MeshData {
        match self {
            Self::Triangle => triangle(),
            Self::Rect => rect(),
            Self::Floor => floor(),
            Self::Cube => cube(),
            Self::UvSphere => UvSphereMesh::mesh_data(),
        }
    }
}

/// 固定分辨率的 UV sphere 生成器。
///
/// 球体直径固定为 1，实例通过 transform 调整大小。经度接缝保留两列顶点，
/// 极点按扇区复制并只生成一个非退化三角形，从而让 UV、法线和切线在上传前保持
/// 与普通 `SubmeshData` 相同的数组约束。
struct UvSphereMesh;

impl UvSphereMesh {
    const LONGITUDE_SEGMENTS: usize = 64;
    const LATITUDE_SEGMENTS: usize = 32;
    const RADIUS: f32 = 0.5;

    fn mesh_data() -> MeshData {
        let columns = Self::LONGITUDE_SEGMENTS + 1;
        let rows = Self::LATITUDE_SEGMENTS + 1;
        let vertex_count = columns * rows;
        let mut positions = Vec::with_capacity(vertex_count);
        let mut normals = Vec::with_capacity(vertex_count);
        let mut tangents = Vec::with_capacity(vertex_count);
        let mut tex_coords = Vec::with_capacity(vertex_count);

        for latitude in 0..=Self::LATITUDE_SEGMENTS {
            let v = latitude as f32 / Self::LATITUDE_SEGMENTS as f32;
            let theta = std::f32::consts::PI * v;
            let sin_theta = theta.sin();
            let cos_theta = theta.cos();

            for longitude in 0..=Self::LONGITUDE_SEGMENTS {
                let u = longitude as f32 / Self::LONGITUDE_SEGMENTS as f32;
                let phi = std::f32::consts::TAU * u;
                let sin_phi = phi.sin();
                let cos_phi = phi.cos();
                let normal = glam::vec3(sin_theta * cos_phi, cos_theta, sin_theta * sin_phi);

                positions.push(normal * Self::RADIUS);
                normals.push(normal);
                tangents.push(glam::vec4(-sin_phi, 0.0, cos_phi, 1.0));
                tex_coords.push(glam::vec2(u, v));
            }
        }

        let mut indices = Vec::with_capacity(Self::LATITUDE_SEGMENTS * Self::LONGITUDE_SEGMENTS * 6);
        for latitude in 0..Self::LATITUDE_SEGMENTS {
            for longitude in 0..Self::LONGITUDE_SEGMENTS {
                let a = (latitude * columns + longitude) as u32;
                let a_next = a + 1;
                let b = ((latitude + 1) * columns + longitude) as u32;
                let b_next = b + 1;

                if latitude == 0 {
                    // 顶部每个扇区只保留 [a_next, b_next, b]，避免同一极点
                    // 的重复顶点产生零面积三角形。
                    indices.extend([a_next, b_next, b]);
                } else if latitude + 1 == Self::LATITUDE_SEGMENTS {
                    // 底部同理只保留 [a, a_next, b]。
                    indices.extend([a, a_next, b]);
                } else {
                    indices.extend([a, a_next, b, a_next, b_next, b]);
                }
            }
        }

        MeshData::from_single_submesh(SubmeshData {
            positions,
            normals,
            tangents,
            tex_coords: vec![tex_coords],
            tangent_tex_coord: 0,
            indices,
            name: ProceduralMeshKind::UvSphere.name().to_string(),
        })
    }
}

/// 位于 XY 平面、法线 +Z 的正立三角形。
pub fn triangle() -> MeshData {
    MeshData::from_single_submesh(SubmeshData {
        positions: vec![
            glam::vec3(-1.0, -1.0, 0.0),
            glam::vec3(1.0, -1.0, 0.0),
            glam::vec3(0.0, 1.0, 0.0),
        ],
        normals: vec![glam::vec3(0.0, 0.0, 1.0); 3],
        tangents: vec![glam::Vec4::ZERO; 3],
        tex_coords: vec![vec![glam::vec2(0.0, 1.0), glam::vec2(1.0, 1.0), glam::vec2(0.5, 0.0)]],
        tangent_tex_coord: 0,
        indices: vec![0, 1, 2],
        name: ProceduralMeshKind::Triangle.name().to_string(),
    })
}

/// 位于 XY 平面、法线 +Z 的矩形。
pub fn rect() -> MeshData {
    MeshData::from_single_submesh(SubmeshData {
        positions: vec![
            glam::vec3(-1.0, 1.0, 0.0),
            glam::vec3(1.0, 1.0, 0.0),
            glam::vec3(1.0, -1.0, 0.0),
            glam::vec3(-1.0, -1.0, 0.0),
        ],
        normals: vec![glam::vec3(0.0, 0.0, 1.0); 4],
        tangents: vec![glam::Vec4::ZERO; 4],
        tex_coords: vec![vec![
            glam::vec2(0.0, 1.0),
            glam::vec2(1.0, 1.0),
            glam::vec2(1.0, 0.0),
            glam::vec2(0.0, 0.0),
        ]],
        tangent_tex_coord: 0,
        indices: vec![0, 1, 2, 0, 2, 3],
        name: ProceduralMeshKind::Rect.name().to_string(),
    })
}

/// 位于 XZ 平面、朝向 +Y 的地面面片。
pub fn floor() -> MeshData {
    MeshData::from_single_submesh(SubmeshData {
        positions: vec![
            glam::vec3(1.0, 0.0, 1.0),
            glam::vec3(1.0, 0.0, -1.0),
            glam::vec3(-1.0, 0.0, -1.0),
            glam::vec3(-1.0, 0.0, 1.0),
        ],
        normals: vec![glam::vec3(0.0, 1.0, 0.0); 4],
        tangents: vec![glam::Vec4::ZERO; 4],
        tex_coords: vec![vec![
            glam::vec2(1.0, 0.0),
            glam::vec2(0.0, 0.0),
            glam::vec2(0.0, 1.0),
            glam::vec2(1.0, 1.0),
        ]],
        tangent_tex_coord: 0,
        indices: vec![0, 1, 2, 0, 2, 3],
        name: ProceduralMeshKind::Floor.name().to_string(),
    })
}

/// 单位 cube，右手系，X 向右，Y 向上。
pub fn cube() -> MeshData {
    MeshData::from_single_submesh(SubmeshData {
        positions: vec![
            glam::vec3(0.5, 0.5, -0.5),
            glam::vec3(-0.5, 0.5, -0.5),
            glam::vec3(-0.5, 0.5, 0.5),
            glam::vec3(0.5, 0.5, 0.5),
            glam::vec3(0.5, -0.5, -0.5),
            glam::vec3(-0.5, -0.5, -0.5),
            glam::vec3(-0.5, -0.5, 0.5),
            glam::vec3(0.5, -0.5, 0.5),
            glam::vec3(0.5, 0.5, 0.5),
            glam::vec3(-0.5, 0.5, 0.5),
            glam::vec3(-0.5, -0.5, 0.5),
            glam::vec3(0.5, -0.5, 0.5),
            glam::vec3(0.5, 0.5, -0.5),
            glam::vec3(-0.5, 0.5, -0.5),
            glam::vec3(-0.5, -0.5, -0.5),
            glam::vec3(0.5, -0.5, -0.5),
            glam::vec3(-0.5, 0.5, 0.5),
            glam::vec3(-0.5, 0.5, -0.5),
            glam::vec3(-0.5, -0.5, -0.5),
            glam::vec3(-0.5, -0.5, 0.5),
            glam::vec3(0.5, 0.5, 0.5),
            glam::vec3(0.5, 0.5, -0.5),
            glam::vec3(0.5, -0.5, -0.5),
            glam::vec3(0.5, -0.5, 0.5),
        ],
        normals: cube_normals(),
        tangents: vec![glam::Vec4::ZERO; 24],
        tex_coords: vec![cube_uvs()],
        tangent_tex_coord: 0,
        indices: vec![
            0, 1, 2, 0, 2, 3, 4, 6, 5, 4, 7, 6, 8, 9, 10, 8, 10, 11, 12, 14, 13, 12, 15, 14, 16, 17, 18, 16, 18, 19,
            20, 22, 21, 20, 23, 22,
        ],
        name: ProceduralMeshKind::Cube.name().to_string(),
    })
}

fn cube_normals() -> Vec<glam::Vec3> {
    [
        glam::vec3(0.0, 1.0, 0.0),
        glam::vec3(0.0, -1.0, 0.0),
        glam::vec3(0.0, 0.0, 1.0),
        glam::vec3(0.0, 0.0, -1.0),
        glam::vec3(-1.0, 0.0, 0.0),
        glam::vec3(1.0, 0.0, 0.0),
    ]
    .into_iter()
    .flat_map(|normal| [normal; 4])
    .collect()
}

fn cube_uvs() -> Vec<glam::Vec2> {
    let face_uvs = [
        glam::vec2(1.0, 0.0),
        glam::vec2(0.0, 0.0),
        glam::vec2(0.0, 1.0),
        glam::vec2(1.0, 1.0),
    ];
    (0..6).flat_map(|_| face_uvs).collect()
}
