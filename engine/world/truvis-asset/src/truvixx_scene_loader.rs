use std::path::PathBuf;

use truvis_cxx_binding::truvixx;

use crate::asset_loader::{LoadResult, SceneLoadRequest};
use crate::handle::{MeshData, RawMaterialData, RawSceneData, RawSceneInstanceData};

/// 实际的 scene 导入任务。
///
/// 只复制 owned CPU 数据，不把 `TruvixxSceneHandle` 或 raw pointer 传回 Rust runtime。
/// panic 会被转换为失败结果，避免后台导入异常越过 `AssetHub` 的状态机边界。
pub(crate) fn load_scene_task(req: SceneLoadRequest) -> LoadResult {
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

fn load_scene_task_inner(path: &PathBuf) -> Result<RawSceneData, String> {
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
    if unsafe { truvixx::truvixx_scene_is_loaded(scene.handle) } != truvixx::ResType_ResTypeSuccess {
        return Err(unsafe { scene_import_error(scene.handle) });
    }

    unsafe { copy_scene_data(scene.handle, path) }
}

/// 读取 C++ importer 保存的最近错误。
///
/// # Safety
///
/// `scene_handle` 必须是当前线程仍然有效的 `TruvixxSceneHandle`，并且其生命周期至少
/// 覆盖本函数内的 `truvixx_scene_last_error` 调用。返回值会立即复制为 Rust `String`，
/// 不把 C 字符串指针传出 FFI 边界。
unsafe fn scene_import_error(scene_handle: truvixx::TruvixxSceneHandle) -> String {
    let error = unsafe { truvixx::truvixx_scene_last_error(scene_handle) };
    if error.is_null() {
        return "scene import failed without error detail".to_string();
    }

    let error = unsafe { std::ffi::CStr::from_ptr(error) }.to_string_lossy().into_owned();
    if error.is_empty() { "scene import failed without error detail".to_string() } else { error }
}

struct TruvixxSceneGuard {
    handle: truvixx::TruvixxSceneHandle,
}

impl Drop for TruvixxSceneGuard {
    /// 释放 C++ scene handle。
    ///
    /// 所有需要跨出后台任务的数据都必须在 guard drop 之前复制到 Rust owned buffer。
    fn drop(&mut self) {
        let _span = tracy_client::span!("truvixx_scene_free");
        unsafe { truvixx::truvixx_scene_free(self.handle) };
    }
}

/// 从 C++ scene handle 复制完整 scene 数据。
///
/// # Safety
///
/// `scene_handle` 必须来自成功加载的 truvixx scene，并在本函数返回前保持有效。
/// 本函数读取的所有 mesh/material/instance 指针都只能在该 handle 生命周期内使用，
/// 因此返回结构必须只包含 owned Rust 数据或普通索引。
unsafe fn copy_scene_data(
    scene_handle: truvixx::TruvixxSceneHandle,
    source_path: &std::path::Path,
) -> Result<RawSceneData, String> {
    // 这里是 FFI 生命周期边界：所有 mesh/material/instance 数据都必须复制进
    // Rust owned buffer，返回后 `TruvixxSceneGuard` 会释放 C++ scene。
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

    Ok(RawSceneData {
        source_path: source_path.to_path_buf(),
        name: model_name,
        meshes,
        materials,
        instances,
    })
}

/// 从 C++ scene handle 复制一个 mesh 的 CPU 几何数据。
///
/// # Safety
///
/// `scene_handle` 必须有效，且 `mesh_index` 必须由 truvixx importer 报告的 mesh
/// 范围内索引给出。函数会立刻把 C++ 指针指向的顶点和索引数据复制到 `Vec`，
/// 不把借用切片或 raw pointer 返回给调用方。
unsafe fn copy_mesh_data(
    scene_handle: truvixx::TruvixxSceneHandle,
    mesh_index: u32,
    model_name: &str,
) -> Result<MeshData, String> {
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
    // C++ 导入器只在 scene handle 存活期间保证这些指针有效，下面立即复制成 Vec。
    let positions = unsafe { std::slice::from_raw_parts(position_ptr as *const glam::Vec3, vertex_count) };
    let normals = unsafe { std::slice::from_raw_parts(normal_ptr as *const glam::Vec3, vertex_count) };
    let tangents = unsafe { std::slice::from_raw_parts(tangent_ptr as *const glam::Vec3, vertex_count) };
    let uvs = unsafe { std::slice::from_raw_parts(uv_ptr as *const glam::Vec2, vertex_count) };

    let indices_ptr = unsafe { truvixx::truvixx_mesh_get_indices(scene_handle, mesh_index) };
    if indices_ptr.is_null() {
        return Err(format!("mesh {} has no index data", mesh_index));
    }

    let indices = unsafe { std::slice::from_raw_parts(indices_ptr, mesh_info.index_count as usize) };

    Ok(MeshData {
        positions: positions.to_vec(),
        normals: normals.to_vec(),
        tangents: tangents.to_vec(),
        uvs: uvs.to_vec(),
        indices: indices.to_vec(),
        name: format!("{}-{}", model_name, mesh_index),
    })
}

/// 从 C++ scene handle 复制一个 material 的 CPU 参数。
///
/// # Safety
///
/// `scene_handle` 必须有效，且 `material_index` 必须在 importer 报告的 material
/// 范围内。texture path 保持 importer 原始表达，稍后由 `AssetHub` 按 scene 路径解析。
unsafe fn copy_material_data(
    scene_handle: truvixx::TruvixxSceneHandle,
    material_index: u32,
) -> Result<RawMaterialData, String> {
    let mut mat = truvixx::TruvixxMat::default();
    let res = unsafe { truvixx::truvixx_material_get(scene_handle, material_index, &mut mat as *mut _) };
    if res != truvixx::ResType_ResTypeSuccess {
        return Err(format!("failed to get material {}", material_index));
    }

    let diffuse_map = unsafe { std::ffi::CStr::from_ptr(mat.diffuse_map.as_ptr()) }.to_string_lossy().into_owned();
    let normal_map = unsafe { std::ffi::CStr::from_ptr(mat.normal_map.as_ptr()) }.to_string_lossy().into_owned();
    let name = unsafe { std::ffi::CStr::from_ptr(mat.name.as_ptr()) }.to_string_lossy().into_owned();

    // texture 路径保持为导入器原始表达，稍后由 AssetHub 根据 scene 路径统一解析。
    Ok(RawMaterialData {
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

/// 从 C++ instance 复制 prefab instance 记录。
///
/// # Safety
///
/// `scene_handle` 和 `instance` 必须来自同一个有效 truvixx scene。返回值仍使用
/// scene 内部 mesh/material index，避免后台任务直接分配或读取 `AssetHub` handle。
unsafe fn copy_instance_data(
    scene_handle: truvixx::TruvixxSceneHandle,
    instance_index: u32,
    instance: truvixx::TruvixxInstance,
) -> Result<Vec<RawSceneInstanceData>, String> {
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

    // 一个 Assimp node 可能引用多个 mesh，这里拆成多个 prefab instance 记录。
    let instances = mesh_indices
        .into_iter()
        .zip(material_indices)
        .enumerate()
        .map(|(submesh_index, (mesh_index, material_index))| RawSceneInstanceData {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_scene_task_reports_importer_error_for_invalid_scene_file() {
        let file_name = format!(
            "truvis-invalid-scene-{}-{}.fbx",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        );
        let path = std::env::temp_dir().join(file_name);
        std::fs::write(&path, b"not a valid scene").unwrap();

        let result = load_scene_task_inner(&path);
        let _ = std::fs::remove_file(&path);

        let error = result.expect_err("invalid scene file should fail import");
        assert!(!error.is_empty());
        assert_ne!(error, "truvixx_scene_load returned null");
        assert_ne!(error, "scene import failed without error detail");
        assert!(error.contains("Assimp error"), "unexpected importer error: {error}");
    }
}
