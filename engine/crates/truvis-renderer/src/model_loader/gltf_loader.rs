//! GLTF 场景加载器
//!
//! 支持 GLTF 2.0 格式（.gltf / .glb），包含 PBR 材质、法线贴图、发光材质。
//! 不支持骨骼动画、稀疏 accessor 和 GPU instancing 扩展。

use std::path::Path;

use glam::{Vec2, Vec3, Vec4};
use truvis_asset::asset_hub::AssetHub;
use truvis_gfx::resources::special_buffers::index_buffer::GfxIndex32Buffer;
use truvis_gfx::resources::vertex_layout::soa_3d::VertexLayoutSoA3D;
use truvis_render_interface::geometry::RtGeometry;
use truvis_scene::components::instance::Instance;
use truvis_scene::components::material::Material;
use truvis_scene::components::mesh::Mesh;
use truvis_scene::guid_new_type::{InstanceHandle, MaterialHandle, MeshHandle};
use truvis_scene::scene_manager::SceneManager;

/// GLTF 场景加载器
///
/// 与 `AssimpSceneLoader` 接口对齐，将 GLTF 场景解析为引擎内部的
/// `Mesh` / `Material` / `Instance` 并注册到 `SceneManager`。
pub struct GltfSceneLoader;

impl GltfSceneLoader {
    /// 加载 GLTF/GLB 文件，返回场景中所有 instance 的 handle 列表。
    pub fn load_scene(
        model_file: &Path,
        scene_manager: &mut SceneManager,
        asset_hub: &mut AssetHub,
    ) -> Vec<InstanceHandle> {
        let (doc, buffers, _images) = gltf::import(model_file)
            .unwrap_or_else(|e| panic!("failed to load gltf file {:?}: {}", model_file, e));

        let model_name = model_file.file_stem().and_then(|s| s.to_str()).unwrap_or("gltf-model");
        let base_dir = model_file.parent().unwrap_or(Path::new(""));

        log::info!("loading gltf scene: {} ({} meshes, {} materials)", model_name, doc.meshes().count(), doc.materials().count());

        // 注册无材质 primitive 使用的默认材质
        let default_mat_handle = scene_manager.register_mat(Material::default());

        // 加载所有显式材质
        let mat_handles: Vec<MaterialHandle> = doc
            .materials()
            .map(|mat| {
                let material = Self::load_material(&mat, base_dir, asset_hub);
                scene_manager.register_mat(material)
            })
            .collect();

        // 加载所有 mesh（每个 primitive 作为一个 RtGeometry）
        let mesh_handles: Vec<(MeshHandle, Vec<Option<usize>>)> = doc
            .meshes()
            .enumerate()
            .map(|(mesh_idx, mesh)| {
                let (mut mesh_data, mat_indices) = Self::load_mesh(&mesh, &buffers, model_name, mesh_idx);
                mesh_data.build_blas();
                let handle = scene_manager.register_mesh(mesh_data);
                (handle, mat_indices)
            })
            .collect();

        // 遍历节点树，为每个含 mesh 的节点创建 instance
        let mut instances = Vec::new();
        let scene = doc.default_scene().or_else(|| doc.scenes().next());
        if let Some(scene) = scene {
            for root_node in scene.nodes() {
                Self::traverse_nodes(
                    &root_node,
                    glam::Mat4::IDENTITY,
                    &mesh_handles,
                    &mat_handles,
                    default_mat_handle,
                    scene_manager,
                    &mut instances,
                );
            }
        }

        log::info!("gltf scene loaded: {} instances", instances.len());
        instances
    }

    /// 解析单个 GLTF mesh，每个 primitive 生成一个 `RtGeometry`。
    ///
    /// 返回 `(Mesh, Vec<Option<material_index>>)`，material index 与 primitive 一一对应。
    fn load_mesh(
        mesh: &gltf::Mesh<'_>,
        buffers: &[gltf::buffer::Data],
        model_name: &str,
        mesh_idx: usize,
    ) -> (Mesh, Vec<Option<usize>>) {
        let mut geometries = Vec::new();
        let mut material_indices = Vec::new();

        for (prim_idx, primitive) in mesh.primitives().enumerate() {
            let reader = primitive.reader(|buffer| Some(&buffers[buffer.index()]));

            let positions: Vec<Vec3> = match reader.read_positions() {
                Some(iter) => iter.map(Vec3::from).collect(),
                None => continue, // 跳过无顶点数据的 primitive
            };
            let vertex_count = positions.len();

            let normals: Vec<Vec3> = reader
                .read_normals()
                .map(|iter| iter.map(Vec3::from).collect())
                .unwrap_or_else(|| vec![Vec3::Y; vertex_count]);

            // GLTF tangent 为 [f32;4]，w 为 bitangent 符号；当前引擎不使用 bitangent，仅取 xyz。
            // 缺失时填充零值以跳过法线贴图效果。
            let tangents: Vec<Vec3> = reader
                .read_tangents()
                .map(|iter| iter.map(|t| Vec3::new(t[0], t[1], t[2])).collect())
                .unwrap_or_else(|| vec![Vec3::ZERO; vertex_count]);

            let uvs: Vec<Vec2> = reader
                .read_tex_coords(0)
                .map(|iter| iter.into_f32().map(Vec2::from).collect())
                .unwrap_or_else(|| vec![Vec2::ZERO; vertex_count]);

            let indices: Vec<u32> = reader
                .read_indices()
                .map(|iter| iter.into_u32().collect())
                .unwrap_or_else(|| (0..vertex_count as u32).collect());

            let buf_name = format!("{}-mesh{}-prim{}", model_name, mesh_idx, prim_idx);
            let vertex_buffer =
                VertexLayoutSoA3D::create_vertex_buffer(&positions, &normals, &tangents, &uvs, &buf_name);

            let index_buffer = GfxIndex32Buffer::new_device_local(indices.len(), format!("{}-idx", buf_name));
            index_buffer.transfer_data_sync(&indices);

            geometries.push(RtGeometry { vertex_buffer, index_buffer });
            material_indices.push(primitive.material().index());
        }

        let mesh_data = Mesh {
            geometries,
            blas: None,
            blas_device_address: None,
            name: format!("{}-mesh{}", model_name, mesh_idx),
        };

        (mesh_data, material_indices)
    }

    /// 将 GLTF material 映射为引擎 `Material`，并触发贴图异步加载。
    fn load_material(material: &gltf::Material<'_>, base_dir: &Path, asset_hub: &mut AssetHub) -> Material {
        let pbr = material.pbr_metallic_roughness();

        let base_color = {
            let c = pbr.base_color_factor();
            Vec4::new(c[0], c[1], c[2], c[3])
        };
        let emissive = {
            let e = material.emissive_factor();
            Vec4::new(e[0], e[1], e[2], 0.0)
        };

        let diffuse_map = pbr
            .base_color_texture()
            .and_then(|info| Self::resolve_texture_path(info.texture().source().source(), base_dir))
            .unwrap_or_default();

        let normal_map = material
            .normal_texture()
            .and_then(|info| Self::resolve_texture_path(info.texture().source().source(), base_dir))
            .unwrap_or_default();

        if !diffuse_map.is_empty() {
            asset_hub.load_texture(std::path::PathBuf::from(&diffuse_map));
        }
        if !normal_map.is_empty() {
            asset_hub.load_texture(std::path::PathBuf::from(&normal_map));
        }

        Material {
            base_color,
            emissive,
            metallic: pbr.metallic_factor(),
            roughness: pbr.roughness_factor(),
            // GLTF Alpha mode：Opaque → 完全不透明；Mask/Blend → 半透明
            opaque: if material.alpha_mode() == gltf::material::AlphaMode::Opaque { 1.0 } else { 0.0 },
            diffuse_map,
            normal_map,
        }
    }

    /// 将 GLTF image source 解析为绝对文件路径。
    ///
    /// 仅支持 URI 形式的外部贴图；内嵌 buffer view 贴图返回 `None`。
    fn resolve_texture_path(source: gltf::image::Source<'_>, base_dir: &Path) -> Option<String> {
        match source {
            gltf::image::Source::Uri { uri, .. } => {
                let path = base_dir.join(uri);
                path.to_str().map(|s| s.to_string())
            }
            gltf::image::Source::View { .. } => None,
        }
    }

    /// 递归遍历 GLTF 节点树，为每个含 mesh 的节点注册 `Instance`。
    ///
    /// GLTF 使用列主序矩阵，与 `glam::Mat4::from_cols_array_2d` 对齐。
    fn traverse_nodes(
        node: &gltf::Node<'_>,
        parent_transform: glam::Mat4,
        mesh_handles: &[(MeshHandle, Vec<Option<usize>>)],
        mat_handles: &[MaterialHandle],
        default_mat_handle: MaterialHandle,
        scene_manager: &mut SceneManager,
        instances: &mut Vec<InstanceHandle>,
    ) {
        let local_transform = glam::Mat4::from_cols_array_2d(&node.transform().matrix());
        let world_transform = parent_transform * local_transform;

        if let Some(mesh) = node.mesh() {
            let (mesh_handle, mat_indices) = &mesh_handles[mesh.index()];

            let materials: Vec<MaterialHandle> = mat_indices
                .iter()
                .map(|opt_idx| {
                    opt_idx
                        .and_then(|idx| mat_handles.get(idx).copied())
                        .unwrap_or(default_mat_handle)
                })
                .collect();

            let instance = Instance { mesh: *mesh_handle, materials, transform: world_transform };
            instances.push(scene_manager.register_instance(instance));
        }

        for child in node.children() {
            Self::traverse_nodes(
                &child,
                world_transform,
                mesh_handles,
                mat_handles,
                default_mat_handle,
                scene_manager,
                instances,
            );
        }
    }
}
