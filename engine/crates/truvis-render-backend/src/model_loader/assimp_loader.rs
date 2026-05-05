use itertools::Itertools;

use truvis_asset::asset_hub::AssetHub;
use truvis_cxx_binding::truvixx;
use truvis_gfx::resources::special_buffers::index_buffer::GfxIndex32Buffer;
use truvis_gfx::resources::vertex_layout::soa_3d::VertexLayoutSoA3D;
use truvis_render_interface::geometry::RtGeometry;
use truvis_scene::components::instance::Instance;
use truvis_scene::components::material::Material;
use truvis_scene::components::mesh::Mesh;
use truvis_scene::guid_new_type::{InstanceHandle, MaterialHandle, MeshHandle};
use truvis_scene::scene_manager::SceneManager;

/// Assimp 场景加载器
///
/// 封装 Assimp 库，提供场景加载功能。支持多种 3D 模型格式（FBX、GLTF、OBJ 等）。
///
/// # 使用示例
/// ```ignore
/// let instances = AssimpSceneLoader::load_scene(
///     Path::new("model.fbx"),
///     |ins| scene_manager.register_instance(ins),
///     |mesh| scene_manager.register_mesh(mesh),
///     |mat| scene_manager.register_material(mat),
/// );
/// ```
pub struct AssimpSceneLoader {
    scene_handle: truvixx::TruvixxSceneHandle,
    model_name: String,

    meshes: Vec<MeshHandle>,
    mats: Vec<MaterialHandle>,
    instances: Vec<InstanceHandle>,
}

impl AssimpSceneLoader {
    /// # return
    /// 返回整个场景的所有 instance id
    pub fn load_scene(
        model_file: &std::path::Path,
        scene_manager: &mut SceneManager,
        asset_hub: &mut AssetHub,
    ) -> Vec<InstanceHandle> {
        let _span = tracy_client::span!("AssimpSceneLoader::load_scene");

        let model_file = model_file.to_str().unwrap();
        let c_model_file = std::ffi::CString::new(model_file).unwrap();

        let loader = unsafe {
            let _span = tracy_client::span!("truvixx_scene_load");
            truvixx::truvixx_scene_load(c_model_file.as_ptr())
        };
        let model_name = model_file.split('/').next_back().unwrap();

        let mut scene_loader = AssimpSceneLoader {
            scene_handle: loader,
            model_name: model_name.to_string(),
            meshes: vec![],
            mats: vec![],
            instances: vec![],
        };

        scene_loader.load_mesh(|mut mesh| {
            mesh.build_blas();
            scene_manager.register_mesh(mesh)
        });
        scene_loader.load_mats(|mat| {
            if !mat.diffuse_map.is_empty() {
                asset_hub.load_texture(std::path::PathBuf::from(&mat.diffuse_map));
            }
            scene_manager.register_mat(mat)
        });
        scene_loader.load_instance(|ins| scene_manager.register_instance(ins));

        {
            let _span = tracy_client::span!("truvixx_scene_free");
            unsafe { truvixx::truvixx_scene_free(loader) };
        }

        scene_loader.instances
    }

    unsafe fn create_mesh(scene_handle: truvixx::TruvixxSceneHandle, mesh_idx: u32, model_name: &str) -> Mesh {
        unsafe {
            let mut mesh_info = truvixx::TruvixxMeshInfo::default();
            let res = truvixx::truvixx_mesh_get_info(scene_handle, mesh_idx, &mut mesh_info as *mut _);
            if res != truvixx::ResType_ResTypeSuccess {
                panic!("Failed to get mesh info for mesh {}", mesh_idx);
            }

            let position_ptr = truvixx::truvixx_mesh_get_positions(scene_handle, mesh_idx);
            let normal_ptr = truvixx::truvixx_mesh_get_normals(scene_handle, mesh_idx);
            let tangent_ptr = truvixx::truvixx_mesh_get_tangents(scene_handle, mesh_idx);
            let uv_ptr = truvixx::truvixx_mesh_get_uvs(scene_handle, mesh_idx);
            if position_ptr.is_null() || normal_ptr.is_null() || tangent_ptr.is_null() || uv_ptr.is_null() {
                panic!("Mesh {} is missing vertex attributes", mesh_idx);
            }

            let positions =
                std::slice::from_raw_parts(position_ptr as *const glam::Vec3, mesh_info.vertex_count as usize);
            let normals = std::slice::from_raw_parts(normal_ptr as *const glam::Vec3, mesh_info.vertex_count as usize);
            let tangents =
                std::slice::from_raw_parts(tangent_ptr as *const glam::Vec3, mesh_info.vertex_count as usize);
            let uvs = std::slice::from_raw_parts(uv_ptr as *const glam::Vec2, mesh_info.vertex_count as usize);

            let vertex_buffer = VertexLayoutSoA3D::create_vertex_buffer(
                positions,
                normals,
                tangents,
                uvs,
                format!("{}-mesh-{}", model_name, mesh_idx),
            );

            let indices_ptr = truvixx::truvixx_mesh_get_indices(scene_handle, mesh_idx);
            if indices_ptr.is_null() {
                panic!("Mesh {} has no index data", mesh_idx);
            }

            let indices = std::slice::from_raw_parts(indices_ptr, mesh_info.index_count as usize);

            let index_buffer =
                GfxIndex32Buffer::new_device_local(indices.len(), format!("{}-mesh-{}-indices", model_name, mesh_idx));
            index_buffer.transfer_data_sync(indices);

            // 只有 single geometry 的 mesh
            Mesh {
                geometries: vec![RtGeometry {
                    vertex_buffer,
                    index_buffer,
                }],
                blas: None,
                blas_device_address: None,
                name: format!("{}-{}", model_name, mesh_idx),
            }
        }
    }

    /// 加载场景中基础的几何体
    fn load_mesh(&mut self, mut mesh_register: impl FnMut(Mesh) -> MeshHandle) {
        let _span = tracy_client::span!("load_mesh");
        let mesh_cnt = unsafe { truvixx::truvixx_scene_mesh_count(self.scene_handle) };

        let mesh_uuids = (0..mesh_cnt)
            .map(|mesh_idx| unsafe {
                let mesh = Self::create_mesh(self.scene_handle, mesh_idx, &self.model_name);
                mesh_register(mesh)
            })
            .collect_vec();

        self.meshes = mesh_uuids;
    }

    unsafe fn create_mat(scene_handle: truvixx::TruvixxSceneHandle, mat_idx: u32) -> Material {
        unsafe {
            let mut mat = truvixx::TruvixxMat::default();
            let res = truvixx::truvixx_material_get(scene_handle, mat_idx, &mut mat as *mut _);
            if res != truvixx::ResType_ResTypeSuccess {
                panic!("Failed to get material {}", mat_idx);
            }

            Material {
                base_color: std::mem::transmute::<truvixx::TruvixxFloat4, glam::Vec4>(mat.base_color),
                emissive: std::mem::transmute::<truvixx::TruvixxFloat4, glam::Vec4>(mat.emissive),
                metallic: mat.metallic,
                roughness: mat.roughness,
                opaque: mat.opacity,

                diffuse_map: std::ffi::CStr::from_ptr(mat.diffuse_map.as_ptr()).to_str().unwrap().to_string(),
                normal_map: std::ffi::CStr::from_ptr(mat.normal_map.as_ptr()).to_str().unwrap().to_string(),
            }
        }
    }

    /// 加载场景中的所有材质
    fn load_mats(&mut self, mut mat_register: impl FnMut(Material) -> MaterialHandle) {
        let _span = tracy_client::span!("load_mats");
        let mat_cnt = unsafe { truvixx::truvixx_scene_material_count(self.scene_handle) };

        let mat_uuids = (0..mat_cnt)
            .map(|mat_idx| unsafe {
                let mat = Self::create_mat(self.scene_handle, mat_idx);
                mat_register(mat)
            })
            .collect_vec();

        self.mats = mat_uuids;
    }

    unsafe fn create_instance(&self, instance_idx: u32, instance: truvixx::TruvixxInstance) -> Vec<Instance> {
        let mut mesh_indices = vec![0_u32; instance.mesh_count as usize];
        let mut mat_indices = vec![0_u32; instance.mesh_count as usize];

        let res = unsafe {
            truvixx::truvixx_instance_get_refs(
                self.scene_handle,
                instance_idx,
                mesh_indices.as_mut_ptr(),
                mat_indices.as_mut_ptr(),
            )
        };
        if res != truvixx::ResType_ResTypeSuccess {
            panic!("Failed to get instance {} refs", instance_idx);
        }

        let mesh_uuids = mesh_indices.iter().map(|mesh_idx| self.meshes[*mesh_idx as usize]);
        let mat_uuids = mat_indices.iter().map(|mat_idx| self.mats[*mat_idx as usize]);

        std::iter::zip(mesh_uuids, mat_uuids)
            .map(|(mesh_uuid, mat_uuid)| Instance {
                transform: unsafe {
                    std::mem::transmute::<truvixx::TruvixxFloat4x4, glam::Mat4>(instance.world_transform)
                },
                mesh: mesh_uuid,
                materials: vec![mat_uuid],
            })
            .collect_vec()
    }

    /// 加载场景中的所有 instance
    ///
    /// 由于 Assimp 的复用层级是 geometry，而应用需要的复用层级是 mesh
    ///
    /// 因此将 Assimp 中的一个 Instance 拆分为多个 Instance，将其 geometry
    /// 提升为 mesh
    fn load_instance(&mut self, instance_register: impl FnMut(Instance) -> InstanceHandle) {
        let _span = tracy_client::span!("load_instance");
        let instance_cnt = unsafe { truvixx::truvixx_scene_instance_count(self.scene_handle) };
        let instances = (0..instance_cnt)
            .filter_map(|instance_idx| {
                let mut instance = truvixx::TruvixxInstance::default();
                let res =
                    unsafe { truvixx::truvixx_instance_get(self.scene_handle, instance_idx, &mut instance as *mut _) };
                if res != truvixx::ResType_ResTypeSuccess {
                    panic!("Failed to get instance {}", instance_idx);
                }

                // 排除空间点，比如 camera, light
                if instance.mesh_count == 0 { None } else { Some((instance_idx, instance)) }
            })
            .flat_map(|(instance_idx, instance_info)| unsafe {
                self.create_instance(instance_idx, instance_info).into_iter()
            })
            .map(instance_register)
            .collect_vec();

        self.instances = instances
    }
}
