//! glTF scene 导入任务。
//!
//! 本模块运行在 asset 后台线程中，职责只到“从 glTF 文件复制出 owned CPU 数据”。
//! 它不分配 asset handle、不创建 GPU resource，也不把 glTF crate 的借用对象传出任务。
//! 返回给 `AssetHub` 的数据必须保持为 `RawSceneData` 这套现有边界格式，后续 texture
//! 路径解析、mesh/material handle 分配和加载事件生成仍统一收敛在 `AssetHub::update()`。

use std::path::{Path, PathBuf};

use gltf::buffer;

use crate::asset_loader::{LoadResult, ModelLoadRequest};
use crate::handle::{MeshData, RawMaterialData, RawSceneData, RawSceneInstanceData};

/// 实际的 glTF scene 导入任务。
///
/// panic 会被转换为失败结果，避免后台导入异常越过 `AssetHub` 的状态机边界。
/// `req.handle` 只用于把结果关联回 `AssetHub` 已经分配的 model asset，不参与文件读取。
pub(crate) fn load_gltf_scene_task(req: ModelLoadRequest) -> LoadResult {
    let _span = tracy_client::span!("load_gltf_scene_task");
    log::info!("Loading glTF scene: {:?}", req.path);

    let result = std::panic::catch_unwind(|| GltfSceneReader::load_path(&req.path))
        .map_err(|_| "glTF scene import task panicked".to_string())
        .and_then(|result| result);

    match result {
        Ok(data) => LoadResult::ModelSuccess {
            handle: req.handle,
            data,
        },
        Err(error) => {
            log::error!("Failed to load glTF scene {:?}: {}", req.path, error);
            LoadResult::ModelFailure(req.handle, error)
        }
    }
}

/// glTF scene 的只读复制器。
///
/// Reader 拥有一次 `gltf::import` 返回的 document/buffer/image 数据，只在本后台任务内
/// 借用它们读取 primitive、material 和 node tree。所有输出都立即复制到 Rust owned
/// Vec/String/PathBuf，确保任务结束后不会留下 glTF crate 内部借用或 decoded image bytes。
struct GltfSceneReader {
    document: gltf::Document,
    buffers: Vec<buffer::Data>,
    source_path: PathBuf,
    model_name: String,
}

impl GltfSceneReader {
    /// 加载一个 glTF / GLB 文件并复制成 `RawSceneData`。
    ///
    /// `gltf::import` 可以读取外部 buffer 和 GLB 内嵌 buffer；本 v1 仅把外部 image URI
    /// 转成 texture path，GLB/data URI 贴图暂不注册 texture handle，避免改变 AssetHub
    /// 当前以 path 为 texture 内容身份的状态机。
    fn load_path(path: &Path) -> Result<RawSceneData, String> {
        if !path.exists() {
            return Err(format!("glTF scene file does not exist: {:?}", path));
        }

        let (document, buffers, _images) = gltf::import(path).map_err(|err| err.to_string())?;
        let reader = Self {
            document,
            buffers,
            source_path: path.to_path_buf(),
            model_name: Self::model_name(path),
        };

        reader.copy_scene()
    }

    /// 生成 scene 级默认名称。
    fn model_name(source_path: &Path) -> String {
        source_path.file_name().and_then(|name| name.to_str()).unwrap_or("scene").to_string()
    }

    /// 复制完整 glTF scene 数据。
    ///
    /// glTF material 绑定在 primitive 上，而当前 AssetHub 的 `ModelInstanceData` 是
    /// mesh + material handle 组合；因此这里把每个 primitive 扁平化为一个 `MeshData`，
    /// 再让引用它的 node 生成对应的 prefab instance。
    fn copy_scene(&self) -> Result<RawSceneData, String> {
        let mut materials = Vec::new();
        let mut material_index_by_gltf_index = Vec::new();
        for material in self.document.materials() {
            material_index_by_gltf_index.push(materials.len() as u32);
            materials.push(self.copy_material(material));
        }
        let default_material_index = materials.len() as u32;
        materials.push(Self::default_material());

        let mut meshes = Vec::new();
        let mut primitive_refs = Vec::new();
        for mesh in self.document.meshes() {
            let mesh_name = mesh.name().unwrap_or(&self.model_name);
            let mut refs = Vec::new();
            for primitive in mesh.primitives() {
                let mesh_index = meshes.len() as u32;
                let material_index =
                    Self::primitive_material_index(&material_index_by_gltf_index, &primitive, default_material_index);
                let primitive_name = format!("{}-mesh{}-prim{}", mesh_name, mesh.index(), primitive.index());
                meshes.push(self.copy_primitive_mesh(&primitive, primitive_name)?);
                refs.push(GltfPrimitiveRef {
                    mesh_index,
                    material_index,
                });
            }
            primitive_refs.push(refs);
        }

        let mut instances = Vec::new();
        let root_scene = self
            .document
            .default_scene()
            .or_else(|| self.document.scenes().next())
            .ok_or_else(|| format!("glTF scene {:?} contains no scenes", self.source_path))?;
        for node in root_scene.nodes() {
            self.copy_node_instances(&node, glam::Mat4::IDENTITY, &primitive_refs, &mut instances)?;
        }

        Ok(RawSceneData {
            source_path: self.source_path.clone(),
            name: self.model_name.clone(),
            meshes,
            materials,
            instances,
        })
    }

    /// 将 glTF material 复制到 AssetHub 的 raw material 边界格式。
    ///
    /// v1 只读取现有 `MaterialData` 能表达的 PBR metallic-roughness 参数和两类贴图。
    /// 外部 URI 保留为 importer 原始表达，稍后仍由 `AssetHub` 根据 scene 路径统一解析。
    fn copy_material(&self, material: gltf::Material<'_>) -> RawMaterialData {
        let pbr = material.pbr_metallic_roughness();
        let base_color = pbr.base_color_factor();
        let emissive = material.emissive_factor();

        RawMaterialData {
            base_color: glam::Vec4::new(base_color[0], base_color[1], base_color[2], base_color[3]),
            emissive: glam::Vec4::new(emissive[0], emissive[1], emissive[2], 1.0),
            metallic: pbr.metallic_factor(),
            roughness: pbr.roughness_factor(),
            opaque: Self::material_opacity(material.alpha_mode(), base_color[3]),
            diffuse_texture_path: pbr
                .base_color_texture()
                .and_then(|texture| Self::external_texture_path(texture.texture().source())),
            normal_texture_path: material
                .normal_texture()
                .and_then(|texture| Self::external_texture_path(texture.texture().source())),
            name: material.name().map(str::to_string).unwrap_or_else(|| Self::material_fallback_name(material.index())),
        }
    }

    /// 复制一个 primitive 的 CPU mesh 数据。
    ///
    /// 当前 render-side mesh manager 要求 position / normal / tangent / uv 数组长度一致且
    /// index count 为 3 的倍数。glTF 允许部分属性缺失，因此这里按 v1 策略补齐默认值，
    /// 让缺属性模型仍能进入既有上传和 BLAS 构建路径。
    fn copy_primitive_mesh(&self, primitive: &gltf::Primitive<'_>, name: String) -> Result<MeshData, String> {
        if primitive.mode() != gltf::mesh::Mode::Triangles {
            return Err(format!("glTF primitive '{}' is not triangle list", name));
        }

        let reader = primitive.reader(|buffer| self.buffers.get(buffer.index()).map(|data| data.0.as_slice()));
        let positions = reader
            .read_positions()
            .ok_or_else(|| format!("glTF primitive '{}' is missing POSITION attribute", name))?
            .map(|pos| glam::Vec3::new(pos[0], pos[1], pos[2]))
            .collect::<Vec<_>>();

        let vertex_count = positions.len();
        if vertex_count == 0 {
            return Err(format!("glTF primitive '{}' has no vertices", name));
        }

        let indices = reader
            .read_indices()
            .map(|iter| iter.into_u32().collect())
            .unwrap_or_else(|| (0..vertex_count as u32).collect::<Vec<_>>());
        let normals = reader
            .read_normals()
            .map(|iter| iter.map(|normal| glam::Vec3::new(normal[0], normal[1], normal[2])).collect())
            .unwrap_or_else(|| Self::generate_normals(&positions, &indices));
        let tangents = reader
            .read_tangents()
            .map(|iter| iter.map(|tangent| glam::Vec3::new(tangent[0], tangent[1], tangent[2])).collect())
            .unwrap_or_else(|| vec![glam::Vec3::X; vertex_count]);
        let uvs = reader
            .read_tex_coords(0)
            .map(|iter| iter.into_f32().map(|uv| glam::Vec2::new(uv[0], 1.0 - uv[1])).collect())
            .unwrap_or_else(|| vec![glam::Vec2::ZERO; vertex_count]);

        Self::validate_mesh_attributes(&name, vertex_count, &normals, &tangents, &uvs, &indices)?;

        Ok(MeshData {
            positions,
            normals,
            tangents,
            uvs,
            indices,
            name,
        })
    }

    /// 递归复制 node tree 中的 prefab instance。
    ///
    /// transform 累积顺序保持为 parent * local，输出的是 world transform。node 本身引用
    /// mesh 时，每个 primitive-ref 都拆成一条 instance，和现有 Assimp loader 对多 mesh
    /// node 的处理方式一致。
    fn copy_node_instances(
        &self,
        node: &gltf::Node<'_>,
        parent_transform: glam::Mat4,
        primitive_refs: &[Vec<GltfPrimitiveRef>],
        instances: &mut Vec<RawSceneInstanceData>,
    ) -> Result<(), String> {
        let transform = parent_transform * Self::mat4_from_gltf_transform(node.transform());

        if let Some(mesh) = node.mesh() {
            let refs = primitive_refs
                .get(mesh.index())
                .ok_or_else(|| format!("glTF node references missing mesh {}", mesh.index()))?;
            let node_name = node.name().unwrap_or("node");
            for (primitive_index, primitive_ref) in refs.iter().enumerate() {
                instances.push(RawSceneInstanceData {
                    mesh_index: primitive_ref.mesh_index,
                    material_indices: vec![primitive_ref.material_index],
                    transform,
                    name: format!("{}-{}", node_name, primitive_index),
                });
            }
        }

        for child in node.children() {
            self.copy_node_instances(&child, transform, primitive_refs, instances)?;
        }

        Ok(())
    }

    fn primitive_material_index(
        material_index_by_gltf_index: &[u32],
        primitive: &gltf::Primitive<'_>,
        default_material_index: u32,
    ) -> u32 {
        primitive
            .material()
            .index()
            .and_then(|index| material_index_by_gltf_index.get(index).copied())
            .unwrap_or(default_material_index)
    }

    fn default_material() -> RawMaterialData {
        RawMaterialData {
            base_color: glam::Vec4::ONE,
            emissive: glam::Vec4::new(0.0, 0.0, 0.0, 1.0),
            metallic: 0.0,
            roughness: 0.5,
            opaque: 1.0,
            diffuse_texture_path: None,
            normal_texture_path: None,
            name: "material-default".to_string(),
        }
    }

    fn external_texture_path(image: gltf::image::Image<'_>) -> Option<PathBuf> {
        match image.source() {
            gltf::image::Source::Uri { uri, .. } if !uri.starts_with("data:") => Some(PathBuf::from(uri)),
            gltf::image::Source::Uri { .. } | gltf::image::Source::View { .. } => None,
        }
    }

    fn material_opacity(alpha_mode: gltf::material::AlphaMode, base_alpha: f32) -> f32 {
        match alpha_mode {
            gltf::material::AlphaMode::Opaque => 1.0,
            gltf::material::AlphaMode::Mask | gltf::material::AlphaMode::Blend => base_alpha,
        }
    }

    fn material_fallback_name(index: Option<usize>) -> String {
        index.map(|index| format!("material-{}", index)).unwrap_or_else(|| "material-default".to_string())
    }

    fn mat4_from_gltf_transform(transform: gltf::scene::Transform) -> glam::Mat4 {
        let matrix = transform.matrix();
        glam::Mat4::from_cols_array(&[
            matrix[0][0],
            matrix[0][1],
            matrix[0][2],
            matrix[0][3],
            matrix[1][0],
            matrix[1][1],
            matrix[1][2],
            matrix[1][3],
            matrix[2][0],
            matrix[2][1],
            matrix[2][2],
            matrix[2][3],
            matrix[3][0],
            matrix[3][1],
            matrix[3][2],
            matrix[3][3],
        ])
    }

    fn generate_normals(positions: &[glam::Vec3], indices: &[u32]) -> Vec<glam::Vec3> {
        let mut normals = vec![glam::Vec3::ZERO; positions.len()];

        for tri in indices.chunks_exact(3) {
            let i0 = tri[0] as usize;
            let i1 = tri[1] as usize;
            let i2 = tri[2] as usize;
            let (Some(&p0), Some(&p1), Some(&p2)) = (positions.get(i0), positions.get(i1), positions.get(i2)) else {
                continue;
            };
            let face_normal = (p1 - p0).cross(p2 - p0);
            if face_normal.length_squared() <= f32::EPSILON {
                continue;
            }
            normals[i0] += face_normal;
            normals[i1] += face_normal;
            normals[i2] += face_normal;
        }

        normals
            .into_iter()
            .map(|normal| if normal.length_squared() > f32::EPSILON { normal.normalize() } else { glam::Vec3::Y })
            .collect()
    }

    fn validate_mesh_attributes(
        name: &str,
        vertex_count: usize,
        normals: &[glam::Vec3],
        tangents: &[glam::Vec3],
        uvs: &[glam::Vec2],
        indices: &[u32],
    ) -> Result<(), String> {
        if normals.len() != vertex_count || tangents.len() != vertex_count || uvs.len() != vertex_count {
            return Err(format!("glTF primitive '{}' has mismatched vertex attribute counts", name));
        }
        if indices.is_empty() {
            return Err(format!("glTF primitive '{}' has no indices", name));
        }
        if !indices.len().is_multiple_of(3) {
            return Err(format!("glTF primitive '{}' index count is not a multiple of 3", name));
        }
        if indices.iter().any(|&index| index as usize >= vertex_count) {
            return Err(format!("glTF primitive '{}' has out-of-range index", name));
        }

        Ok(())
    }
}

#[derive(Clone, Copy)]
struct GltfPrimitiveRef {
    mesh_index: u32,
    material_index: u32,
}
