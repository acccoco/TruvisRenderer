use std::path::PathBuf;

use truvis_asset::handle::{LoadedMeshData, MeshAssetKey};

/// 内置程序化 mesh 类型。
///
/// 这些数据只描述 CPU 侧顶点属性和索引，不创建 GPU buffer 或 BLAS。调用方应通过
/// `AssetHub::register_mesh_data` 注册后进入标准 `AssetMeshUploader` 路径。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ProceduralMeshKind {
    Triangle,
    Rect,
    Floor,
    Cube,
}

impl ProceduralMeshKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Triangle => "procedural-triangle",
            Self::Rect => "procedural-rect",
            Self::Floor => "procedural-floor",
            Self::Cube => "procedural-cube",
        }
    }

    /// 用于注册到 `AssetHub` 的稳定 key。
    pub fn asset_key(self) -> MeshAssetKey {
        MeshAssetKey {
            source_path: PathBuf::from(format!("procedural://{}", self.name())),
            mesh_index: 0,
        }
    }

    pub fn mesh_data(self) -> LoadedMeshData {
        match self {
            Self::Triangle => triangle(),
            Self::Rect => rect(),
            Self::Floor => floor(),
            Self::Cube => cube(),
        }
    }
}

/// 位于 XY 平面、法线 +Z 的正立三角形。
pub fn triangle() -> LoadedMeshData {
    LoadedMeshData {
        positions: vec![
            glam::vec3(-1.0, -1.0, 0.0),
            glam::vec3(1.0, -1.0, 0.0),
            glam::vec3(0.0, 1.0, 0.0),
        ],
        normals: vec![glam::vec3(0.0, 0.0, 1.0); 3],
        tangents: vec![glam::vec3(1.0, 0.0, 0.0); 3],
        uvs: vec![glam::vec2(0.0, 1.0), glam::vec2(1.0, 1.0), glam::vec2(0.5, 0.0)],
        indices: vec![0, 1, 2],
        name: ProceduralMeshKind::Triangle.name().to_string(),
    }
}

/// 位于 XY 平面、法线 +Z 的矩形。
pub fn rect() -> LoadedMeshData {
    LoadedMeshData {
        positions: vec![
            glam::vec3(-1.0, 1.0, 0.0),
            glam::vec3(1.0, 1.0, 0.0),
            glam::vec3(1.0, -1.0, 0.0),
            glam::vec3(-1.0, -1.0, 0.0),
        ],
        normals: vec![glam::vec3(0.0, 0.0, 1.0); 4],
        tangents: vec![glam::vec3(1.0, 0.0, 0.0); 4],
        uvs: vec![
            glam::vec2(0.0, 1.0),
            glam::vec2(1.0, 1.0),
            glam::vec2(1.0, 0.0),
            glam::vec2(0.0, 0.0),
        ],
        indices: vec![0, 1, 2, 0, 2, 3],
        name: ProceduralMeshKind::Rect.name().to_string(),
    }
}

/// 位于 XZ 平面、朝向 +Y 的地面面片。
pub fn floor() -> LoadedMeshData {
    LoadedMeshData {
        positions: vec![
            glam::vec3(1.0, 0.0, 1.0),
            glam::vec3(1.0, 0.0, -1.0),
            glam::vec3(-1.0, 0.0, -1.0),
            glam::vec3(-1.0, 0.0, 1.0),
        ],
        normals: vec![glam::vec3(0.0, 1.0, 0.0); 4],
        tangents: vec![glam::vec3(1.0, 0.0, 0.0); 4],
        uvs: vec![
            glam::vec2(1.0, 0.0),
            glam::vec2(0.0, 0.0),
            glam::vec2(0.0, 1.0),
            glam::vec2(1.0, 1.0),
        ],
        indices: vec![0, 1, 2, 0, 2, 3],
        name: ProceduralMeshKind::Floor.name().to_string(),
    }
}

/// 单位 cube，右手系，X 向右，Y 向上。
pub fn cube() -> LoadedMeshData {
    LoadedMeshData {
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
        tangents: cube_tangents(),
        uvs: cube_uvs(),
        indices: vec![
            0, 1, 2, 0, 2, 3, 4, 6, 5, 4, 7, 6, 8, 9, 10, 8, 10, 11, 12, 14, 13, 12, 15, 14, 16, 17, 18, 16, 18, 19,
            20, 22, 21, 20, 23, 22,
        ],
        name: ProceduralMeshKind::Cube.name().to_string(),
    }
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

fn cube_tangents() -> Vec<glam::Vec3> {
    [
        glam::vec3(1.0, 0.0, 0.0),
        glam::vec3(1.0, 0.0, 0.0),
        glam::vec3(1.0, 0.0, 0.0),
        glam::vec3(1.0, 0.0, 0.0),
        glam::vec3(0.0, 0.0, 1.0),
        glam::vec3(0.0, 0.0, 1.0),
    ]
    .into_iter()
    .flat_map(|tangent| [tangent; 4])
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn procedural_meshes_have_consistent_vertex_attributes() {
        for kind in [
            ProceduralMeshKind::Triangle,
            ProceduralMeshKind::Rect,
            ProceduralMeshKind::Floor,
            ProceduralMeshKind::Cube,
        ] {
            let mesh = kind.mesh_data();
            let vertex_count = mesh.positions.len();

            assert_eq!(mesh.normals.len(), vertex_count);
            assert_eq!(mesh.tangents.len(), vertex_count);
            assert_eq!(mesh.uvs.len(), vertex_count);
            assert!(mesh.indices.iter().all(|&index| index < vertex_count as u32));
            assert_eq!(mesh.name, kind.name());
        }
    }

    #[test]
    fn procedural_mesh_asset_keys_are_stable_and_distinct() {
        let floor = ProceduralMeshKind::Floor.asset_key();
        let rect = ProceduralMeshKind::Rect.asset_key();

        assert_ne!(floor, rect);
        assert_eq!(floor.source_path, PathBuf::from("procedural://procedural-floor"));
        assert_eq!(floor.mesh_index, 0);
    }
}
