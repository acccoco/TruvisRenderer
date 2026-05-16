use std::path::PathBuf;

use ash::vk;
use crossbeam_channel::{Receiver, Sender};
use crossbeam_utils::sync::WaitGroup;
use image::GenericImageView;
use truvis_cxx_binding::truvixx;

use crate::handle::{
    AssetSceneHandle, AssetTextureHandle, LoadedMeshData, LoadedTextureBytes, RawLoadedMaterialData,
    RawLoadedSceneData, RawLoadedSceneInstanceData,
};

pub(crate) struct AssetLoadRequest {
    pub path: PathBuf,
    pub handle: AssetTextureHandle,
    // pub params: AssetParams, // 预留扩展
}

pub(crate) struct SceneLoadRequest {
    pub path: PathBuf,
    pub handle: AssetSceneHandle,
}

pub(crate) enum LoadResult {
    TextureSuccess {
        handle: AssetTextureHandle,
        data: LoadedTextureBytes,
    },
    TextureFailure(AssetTextureHandle, String),
    SceneSuccess {
        handle: AssetSceneHandle,
        data: RawLoadedSceneData,
    },
    SceneFailure(AssetSceneHandle, String),
}

/// 负责管理后台 IO/解码任务。
pub struct AssetLoader {
    pool: rayon::ThreadPool,
    result_sender: Sender<LoadResult>,
    result_receiver: Receiver<LoadResult>,
    wait_group: Option<WaitGroup>,
}

impl Default for AssetLoader {
    fn default() -> Self {
        Self::new()
    }
}

impl AssetLoader {
    pub fn new() -> Self {
        let (res_tx, res_rx) = crossbeam_channel::unbounded::<LoadResult>();

        let pool = rayon::ThreadPoolBuilder::new()
            .thread_name(|index| format!("Asset-Loader-{}", index))
            .build()
            .expect("Failed to create asset loader thread pool");

        Self {
            pool,
            result_sender: res_tx,
            result_receiver: res_rx,
            wait_group: Some(WaitGroup::new()),
        }
    }

    pub(crate) fn request_load(&self, req: AssetLoadRequest) {
        let result_sender = self.result_sender.clone();
        let wg_task = self.wait_group.as_ref().expect("AssetLoader used after drop").clone();
        self.pool.spawn(move || {
            let result = load_texture_task(req);
            let _ = result_sender.send(result);
            drop(wg_task);
        });
    }

    pub(crate) fn request_load_scene(&self, req: SceneLoadRequest) {
        let result_sender = self.result_sender.clone();
        let wg_task = self.wait_group.as_ref().expect("AssetLoader used after drop").clone();
        self.pool.spawn(move || {
            let result = load_scene_task(req);
            let _ = result_sender.send(result);
            drop(wg_task);
        });
    }

    pub(crate) fn try_recv_result(&self) -> Option<LoadResult> {
        self.result_receiver.try_recv().ok()
    }
}

impl Drop for AssetLoader {
    fn drop(&mut self) {
        log::info!("AssetLoader is being dropped, waiting for tasks to complete...");
        if let Some(wait_group) = self.wait_group.take() {
            wait_group.wait();
        }
        log::info!("All asset loading tasks completed.");
    }
}

/// 实际的加载任务 (运行在 Rayon 线程池中)
/// 执行: 文件读取 -> 图片解码 -> 格式转换
fn load_texture_task(req: AssetLoadRequest) -> LoadResult {
    let _span = tracy_client::span!("load_texture_task");
    log::info!("Loading texture: {:?}", req.path);

    let img_result = image::open(&req.path);

    match img_result {
        Ok(img) => {
            let (width, height) = img.dimensions();
            // 强制转换为 RGBA8
            let img = img.into_rgba8();
            let pixels = img.into_raw();

            let data = LoadedTextureBytes {
                pixels,
                extent: vk::Extent3D {
                    width,
                    height,
                    depth: 1,
                },
                format: vk::Format::R8G8B8A8_UNORM, // 目前统一转为 RGBA8
            };

            LoadResult::TextureSuccess {
                handle: req.handle,
                data,
            }
        }
        Err(e) => {
            log::error!("Failed to load texture {:?}: {}", req.path, e);
            LoadResult::TextureFailure(req.handle, e.to_string())
        }
    }
}

/// 实际的 scene 导入任务。
///
/// 只复制 owned CPU 数据，不把 `TruvixxSceneHandle` 或 raw pointer 传回 Rust runtime。
fn load_scene_task(req: SceneLoadRequest) -> LoadResult {
    let _span = tracy_client::span!("load_scene_task");
    log::info!("Loading scene: {:?}", req.path);

    let result = std::panic::catch_unwind(|| load_scene_task_inner(&req.path))
        .map_err(|_| "scene import task panicked".to_string())
        .and_then(|result| result);

    match result {
        Ok(data) => LoadResult::SceneSuccess {
            handle: req.handle,
            data,
        },
        Err(error) => {
            log::error!("Failed to load scene {:?}: {}", req.path, error);
            LoadResult::SceneFailure(req.handle, error)
        }
    }
}

fn load_scene_task_inner(path: &PathBuf) -> Result<RawLoadedSceneData, String> {
    if !path.exists() {
        return Err(format!("scene file does not exist: {:?}", path));
    }

    let model_file_str = path.to_str().ok_or_else(|| format!("scene path is not valid UTF-8: {:?}", path))?;
    let c_model_file = std::ffi::CString::new(model_file_str).map_err(|err| err.to_string())?;
    let scene_handle = unsafe {
        let _span = tracy_client::span!("truvixx_scene_load");
        truvixx::truvixx_scene_load(c_model_file.as_ptr())
    };

    if scene_handle.is_null() {
        return Err("truvixx_scene_load returned null".to_string());
    }

    let scene = TruvixxSceneGuard { handle: scene_handle };
    unsafe { copy_scene_data(scene.handle, path) }
}

struct TruvixxSceneGuard {
    handle: truvixx::TruvixxSceneHandle,
}

impl Drop for TruvixxSceneGuard {
    fn drop(&mut self) {
        let _span = tracy_client::span!("truvixx_scene_free");
        unsafe { truvixx::truvixx_scene_free(self.handle) };
    }
}

unsafe fn copy_scene_data(
    scene_handle: truvixx::TruvixxSceneHandle,
    source_path: &std::path::Path,
) -> Result<RawLoadedSceneData, String> {
    let model_name = source_path.file_name().and_then(|name| name.to_str()).unwrap_or("scene").to_string();

    let mesh_count = unsafe { truvixx::truvixx_scene_mesh_count(scene_handle) };
    let material_count = unsafe { truvixx::truvixx_scene_material_count(scene_handle) };
    let instance_count = unsafe { truvixx::truvixx_scene_instance_count(scene_handle) };

    let mut meshes = Vec::with_capacity(mesh_count as usize);
    for mesh_index in 0..mesh_count {
        meshes.push(unsafe { copy_mesh_data(scene_handle, mesh_index, &model_name)? });
    }

    let mut materials = Vec::with_capacity(material_count as usize);
    for material_index in 0..material_count {
        materials.push(unsafe { copy_material_data(scene_handle, material_index)? });
    }

    let mut instances = Vec::new();
    for instance_index in 0..instance_count {
        let mut instance = truvixx::TruvixxInstance::default();
        let res = unsafe { truvixx::truvixx_instance_get(scene_handle, instance_index, &mut instance as *mut _) };
        if res != truvixx::ResType_ResTypeSuccess {
            return Err(format!("failed to get instance {}", instance_index));
        }

        if instance.mesh_count == 0 {
            continue;
        }

        instances.extend(unsafe { copy_instance_data(scene_handle, instance_index, instance)? });
    }

    Ok(RawLoadedSceneData {
        source_path: source_path.to_path_buf(),
        name: model_name,
        meshes,
        materials,
        instances,
    })
}

unsafe fn copy_mesh_data(
    scene_handle: truvixx::TruvixxSceneHandle,
    mesh_index: u32,
    model_name: &str,
) -> Result<LoadedMeshData, String> {
    let mut mesh_info = truvixx::TruvixxMeshInfo::default();
    let res = unsafe { truvixx::truvixx_mesh_get_info(scene_handle, mesh_index, &mut mesh_info as *mut _) };
    if res != truvixx::ResType_ResTypeSuccess {
        return Err(format!("failed to get mesh info for mesh {}", mesh_index));
    }

    let position_ptr = unsafe { truvixx::truvixx_mesh_get_positions(scene_handle, mesh_index) };
    let normal_ptr = unsafe { truvixx::truvixx_mesh_get_normals(scene_handle, mesh_index) };
    let tangent_ptr = unsafe { truvixx::truvixx_mesh_get_tangents(scene_handle, mesh_index) };
    let uv_ptr = unsafe { truvixx::truvixx_mesh_get_uvs(scene_handle, mesh_index) };
    if position_ptr.is_null() || normal_ptr.is_null() || tangent_ptr.is_null() || uv_ptr.is_null() {
        return Err(format!("mesh {} is missing required vertex attributes", mesh_index));
    }

    let vertex_count = mesh_info.vertex_count as usize;
    let positions = unsafe { std::slice::from_raw_parts(position_ptr as *const glam::Vec3, vertex_count) };
    let normals = unsafe { std::slice::from_raw_parts(normal_ptr as *const glam::Vec3, vertex_count) };
    let tangents = unsafe { std::slice::from_raw_parts(tangent_ptr as *const glam::Vec3, vertex_count) };
    let uvs = unsafe { std::slice::from_raw_parts(uv_ptr as *const glam::Vec2, vertex_count) };

    let indices_ptr = unsafe { truvixx::truvixx_mesh_get_indices(scene_handle, mesh_index) };
    if indices_ptr.is_null() {
        return Err(format!("mesh {} has no index data", mesh_index));
    }

    let indices = unsafe { std::slice::from_raw_parts(indices_ptr, mesh_info.index_count as usize) };

    Ok(LoadedMeshData {
        positions: positions.to_vec(),
        normals: normals.to_vec(),
        tangents: tangents.to_vec(),
        uvs: uvs.to_vec(),
        indices: indices.to_vec(),
        name: format!("{}-{}", model_name, mesh_index),
    })
}

unsafe fn copy_material_data(
    scene_handle: truvixx::TruvixxSceneHandle,
    material_index: u32,
) -> Result<RawLoadedMaterialData, String> {
    let mut mat = truvixx::TruvixxMat::default();
    let res = unsafe { truvixx::truvixx_material_get(scene_handle, material_index, &mut mat as *mut _) };
    if res != truvixx::ResType_ResTypeSuccess {
        return Err(format!("failed to get material {}", material_index));
    }

    let diffuse_map = unsafe { std::ffi::CStr::from_ptr(mat.diffuse_map.as_ptr()) }.to_string_lossy().into_owned();
    let normal_map = unsafe { std::ffi::CStr::from_ptr(mat.normal_map.as_ptr()) }.to_string_lossy().into_owned();
    let name = unsafe { std::ffi::CStr::from_ptr(mat.name.as_ptr()) }.to_string_lossy().into_owned();

    Ok(RawLoadedMaterialData {
        base_color: unsafe { std::mem::transmute::<truvixx::TruvixxFloat4, glam::Vec4>(mat.base_color) },
        emissive: unsafe { std::mem::transmute::<truvixx::TruvixxFloat4, glam::Vec4>(mat.emissive) },
        metallic: mat.metallic,
        roughness: mat.roughness,
        opaque: mat.opacity,
        diffuse_texture_path: (!diffuse_map.is_empty()).then(|| PathBuf::from(diffuse_map)),
        normal_texture_path: (!normal_map.is_empty()).then(|| PathBuf::from(normal_map)),
        name: if name.is_empty() { format!("material-{}", material_index) } else { name },
    })
}

unsafe fn copy_instance_data(
    scene_handle: truvixx::TruvixxSceneHandle,
    instance_index: u32,
    instance: truvixx::TruvixxInstance,
) -> Result<Vec<RawLoadedSceneInstanceData>, String> {
    let mesh_count = instance.mesh_count as usize;
    let mut mesh_indices = vec![0_u32; mesh_count];
    let mut material_indices = vec![0_u32; mesh_count];

    let res = unsafe {
        truvixx::truvixx_instance_get_refs(
            scene_handle,
            instance_index,
            mesh_indices.as_mut_ptr(),
            material_indices.as_mut_ptr(),
        )
    };
    if res != truvixx::ResType_ResTypeSuccess {
        return Err(format!("failed to get instance {} refs", instance_index));
    }

    let instance_name = unsafe { std::ffi::CStr::from_ptr(instance.name.as_ptr()) }.to_string_lossy().into_owned();
    let transform = unsafe { std::mem::transmute::<truvixx::TruvixxFloat4x4, glam::Mat4>(instance.world_transform) };

    let instances = mesh_indices
        .into_iter()
        .zip(material_indices)
        .enumerate()
        .map(|(submesh_index, (mesh_index, material_index))| RawLoadedSceneInstanceData {
            mesh_index,
            material_indices: vec![material_index],
            transform,
            name: if instance_name.is_empty() {
                format!("instance-{}-{}", instance_index, submesh_index)
            } else {
                format!("{}-{}", instance_name, submesh_index)
            },
        })
        .collect();

    Ok(instances)
}
