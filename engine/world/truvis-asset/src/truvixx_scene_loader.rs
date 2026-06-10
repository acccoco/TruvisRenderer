//! truvixx scene 导入任务的 FFI 边界。
//!
//! 本模块运行在 asset 后台线程中，职责只到“从 C++ importer 复制出 owned CPU 数据”。
//! `TruvixxSceneHandle`、C 字符串指针和 mesh attribute raw pointer 都不能跨出本文件；
//! 返回给 `AssetHub` 的 `RawSceneData` 只能包含 Rust 自己拥有的 `Vec`、`PathBuf` 和普通索引。

use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::path::{Path, PathBuf};

use truvis_assimp_binding::truvixx;

use crate::asset_loader::{LoadResult, ModelLoadRequest};
use crate::handle::{MeshData, RawMaterialData, RawSceneData, RawSceneInstanceData};

/// 实际的 scene 导入任务。
///
/// 只复制 owned CPU 数据，不把 `TruvixxSceneHandle` 或 raw pointer 传回 Rust runtime。
/// panic 会被转换为失败结果，避免后台导入异常越过 `AssetHub` 的状态机边界。
/// `req.handle` 只用于把结果关联回 `AssetHub` 已经分配的 model asset，不参与 FFI 调用。
pub(crate) fn load_scene_task(req: ModelLoadRequest) -> LoadResult {
    let _span = tracy_client::span!("load_scene_task");
    log::info!("Loading scene: {:?}", req.path);

    let result = std::panic::catch_unwind(|| TruvixxSceneReader::load_path(&req.path))
        .map_err(|_| "scene import task panicked".to_string())
        .and_then(|result| result);

    match result {
        Ok(data) => LoadResult::ModelSuccess {
            handle: req.handle,
            data,
        },
        Err(error) => {
            log::error!("Failed to load scene {:?}: {}", req.path, error);
            LoadResult::ModelFailure(req.handle, error)
        }
    }
}

/// C++ scene handle 的唯一 owner。
///
/// 该 guard 只在一次后台导入任务内创建和销毁。reader 可以借用它读取 C ABI 数据，
/// 但不能取得所有权；这样 `truvixx_scene_free` 的释放点始终集中在 `Drop` 中。
struct TruvixxSceneGuard {
    /// 非空的 C++ scene opaque handle。
    ///
    /// `TruvixxSceneReader::load_path` 在构造 guard 前已经过滤 null，后续所有 FFI
    /// 读取都依赖该不变量。handle 指向的 importer 内存只在 guard 存活期间有效。
    handle: truvixx::TruvixxSceneHandle,
}

impl Drop for TruvixxSceneGuard {
    /// 释放 C++ scene handle。
    ///
    /// 所有需要跨出后台任务的数据都必须在 guard drop 之前复制到 Rust owned buffer。
    /// Drop 中只做对称释放，不读取 importer 状态，避免释放路径再引入可失败逻辑。
    fn drop(&mut self) {
        let _span = tracy_client::span!("truvixx_scene_free");
        unsafe { truvixx::truvixx_scene_free(self.handle) };
    }
}

/// C++ scene 的只读复制器。
///
/// Reader 不拥有 handle，只借用 `TruvixxSceneGuard`。所有从 C ABI 读取出的 raw pointer
/// 都必须在 reader 方法内立即复制成 Rust owned 数据，不能跨过 guard 生命周期边界。
/// 它也是本文件内 unsafe FFI 读取和 ABI 转换的收敛点，外层任务只关心成功数据或错误文本。
struct TruvixxSceneReader<'a> {
    /// scene handle 的生命周期锚点。
    ///
    /// 只保存 guard 借用而不是裸 handle 所有权，是为了让 reader 的所有读取都显式受
    /// `TruvixxSceneGuard` 的释放边界约束。
    scene: &'a TruvixxSceneGuard,
    /// 原始 scene 路径。
    ///
    /// 该路径会原样写入 `RawSceneData::source_path`，后续由 `AssetHub` 使用它生成
    /// model / mesh / material key，并解析 material 中的相对 texture path。
    source_path: &'a Path,
    /// 当前导入源的显示名。
    ///
    /// mesh fallback name 会复用它，避免每个 mesh 复制时重复读取 path 组件。
    model_name: String,
}

impl TruvixxSceneReader<'_> {
    /// 加载一个 scene 文件并复制成 Rust owned 数据。
    ///
    /// 这里是 C++ importer 进入 Rust asset 状态机的唯一入口：路径校验、C 字符串构造、
    /// scene handle 创建、导入成功检查和最终 CPU 数据复制都按固定顺序完成。
    fn load_path(path: &Path) -> Result<RawSceneData, String> {
        if !path.exists() {
            return Err(format!("scene file does not exist: {:?}", path));
        }

        // C API 接收 UTF-8 C 字符串。CString 只需要覆盖 `truvixx_scene_load` 调用；
        // 返回后的 importer 数据通过 scene handle 持有，不借用这里的 Rust path buffer。
        let model_file_str = path.to_str().ok_or_else(|| format!("scene path is not valid UTF-8: {:?}", path))?;
        let c_model_file = CString::new(model_file_str).map_err(|err| err.to_string())?;
        let scene_handle = unsafe {
            let _span = tracy_client::span!("truvixx_scene_load");
            truvixx::truvixx_scene_load(c_model_file.as_ptr())
        };

        if scene_handle.is_null() {
            return Err("truvixx_scene_load returned null".to_string());
        }

        // guard 从这里开始成为 handle 生命周期 owner。即使后续导入失败并提前返回，
        // guard 也会在离开作用域时释放 C++ scene，避免失败路径泄露 importer 内存。
        let scene = TruvixxSceneGuard { handle: scene_handle };
        let reader = TruvixxSceneReader {
            scene: &scene,
            source_path: path,
            model_name: Self::model_name(path),
        };
        if !reader.is_loaded() {
            return Err(reader.import_error());
        }

        reader.copy_scene()
    }

    /// 生成 scene 级默认名称。
    ///
    /// 只使用文件名部分，不访问文件系统做 canonicalize，保持与 `AssetHub` 当前路径策略一致。
    fn model_name(source_path: &Path) -> String {
        source_path.file_name().and_then(|name| name.to_str()).unwrap_or("scene").to_string()
    }

    /// 读取 reader 当前借用的 C++ scene handle。
    ///
    /// 返回裸 handle 只供本 impl 内部 FFI 调用使用；不要把它保存到任何返回结构中。
    fn handle(&self) -> truvixx::TruvixxSceneHandle {
        self.scene.handle
    }

    /// 查询 C++ importer 是否已经成功完成导入。
    ///
    /// `truvixx_scene_load` 失败时也可能返回非空 scene，以便调用方读取详细错误；
    /// 因此 null 检查之后仍必须显式调用该函数确认状态。
    fn is_loaded(&self) -> bool {
        (unsafe { truvixx::truvixx_scene_is_loaded(self.handle()) }) == truvixx::ResType_ResTypeSuccess
    }

    /// 复制 C++ importer 保存的最近错误。
    ///
    /// 错误字符串由 scene 内部持有，只在 guard 存活期间有效。这里立即复制成 Rust
    /// `String`，并把空字符串统一映射成稳定 fallback 文本。
    fn import_error(&self) -> String {
        let error = unsafe { truvixx::truvixx_scene_last_error(self.handle()) };
        if error.is_null() {
            return "scene import failed without error detail".to_string();
        }

        // 错误字符串由 scene 内部持有；这里立即复制成 Rust String，避免裸指针传出。
        let error = unsafe { CStr::from_ptr(error) }.to_string_lossy().into_owned();
        if error.is_empty() { "scene import failed without error detail".to_string() } else { error }
    }

    /// 复制完整 scene 数据。
    ///
    /// 该函数定义 FFI 数据离开 C++ importer 的总边界：mesh/material/instance 会在这里
    /// 逐项复制成 Rust owned 数据；scene 内部索引仍保持 importer 返回的原始编号，
    /// 稍后由 `AssetHub` 转换成稳定 asset handle。
    fn copy_scene(&self) -> Result<RawSceneData, String> {
        // 这里是 FFI 生命周期边界：所有 mesh/material/instance 数据都必须复制进
        // Rust owned buffer，返回后 `TruvixxSceneGuard` 会释放 C++ scene。
        let mesh_count = unsafe { truvixx::truvixx_scene_mesh_count(self.handle()) };
        let material_count = unsafe { truvixx::truvixx_scene_material_count(self.handle()) };
        let instance_count = unsafe { truvixx::truvixx_scene_instance_count(self.handle()) };

        let mut meshes = Vec::with_capacity(mesh_count as usize);
        for mesh_index in 0..mesh_count {
            meshes.push(self.copy_mesh(mesh_index)?);
        }

        let mut materials = Vec::with_capacity(material_count as usize);
        for material_index in 0..material_count {
            materials.push(self.copy_material(material_index)?);
        }

        let mut instances = Vec::new();
        for instance_index in 0..instance_count {
            instances.extend(self.copy_instance(instance_index)?);
        }

        Ok(RawSceneData {
            source_path: self.source_path.to_path_buf(),
            name: self.model_name.clone(),
            meshes,
            materials,
            instances,
        })
    }

    /// 复制一个 mesh 的 CPU 几何数据。
    ///
    /// C++ 侧返回的 attribute/index pointer 都是 scene 内部存储的非拥有视图，只能在
    /// `TruvixxSceneGuard` 存活期间读取。这里不把它们转换成 `glam` slice，而是先按
    /// C ABI 类型读取，再逐项构造 `glam` 值，避免依赖 `glam` 的内存布局。
    fn copy_mesh(&self, mesh_index: u32) -> Result<MeshData, String> {
        let mut mesh_info = truvixx::TruvixxMeshInfo::default();
        let res = unsafe { truvixx::truvixx_mesh_get_info(self.handle(), mesh_index, &mut mesh_info as *mut _) };
        if res != truvixx::ResType_ResTypeSuccess {
            return Err(format!("failed to get mesh info for mesh {}", mesh_index));
        }

        let position_ptr = unsafe { truvixx::truvixx_mesh_get_positions(self.handle(), mesh_index) };
        let normal_ptr = unsafe { truvixx::truvixx_mesh_get_normals(self.handle(), mesh_index) };
        let tangent_ptr = unsafe { truvixx::truvixx_mesh_get_tangents(self.handle(), mesh_index) };
        let uv_ptr = unsafe { truvixx::truvixx_mesh_get_uvs(self.handle(), mesh_index) };
        if position_ptr.is_null() || normal_ptr.is_null() || tangent_ptr.is_null() || uv_ptr.is_null() {
            return Err(format!("mesh {} is missing required vertex attributes", mesh_index));
        }

        let vertex_count = mesh_info.vertex_count as usize;
        // C++ 导入器只在 scene handle 存活期间保证这些指针有效，下面立即转换并复制成 Vec。
        let positions = unsafe { std::slice::from_raw_parts(position_ptr, vertex_count) }
            .iter()
            .copied()
            .map(Self::truvixx_float3_to_vec3)
            .collect();
        let normals = unsafe { std::slice::from_raw_parts(normal_ptr, vertex_count) }
            .iter()
            .copied()
            .map(Self::truvixx_float3_to_vec3)
            .collect();
        let tangents = unsafe { std::slice::from_raw_parts(tangent_ptr, vertex_count) }
            .iter()
            .copied()
            .map(Self::truvixx_float3_to_vec3)
            .collect();
        let uvs = unsafe { std::slice::from_raw_parts(uv_ptr, vertex_count) }
            .iter()
            .copied()
            .map(Self::truvixx_float2_to_vec2)
            .collect();

        let indices_ptr = unsafe { truvixx::truvixx_mesh_get_indices(self.handle(), mesh_index) };
        if indices_ptr.is_null() {
            return Err(format!("mesh {} has no index data", mesh_index));
        }

        let indices = unsafe { std::slice::from_raw_parts(indices_ptr, mesh_info.index_count as usize) };

        // `MeshData` 是 asset 层传给 render-side mesh manager 的 owned CPU 边界格式。
        // 从这里返回后，C++ importer 的顶点/索引内存是否释放都不再影响 Rust 数据。
        Ok(MeshData {
            positions,
            normals,
            tangents,
            uvs,
            indices: indices.to_vec(),
            name: format!("{}-{}", self.model_name, mesh_index),
        })
    }

    /// 复制一个 material 的 CPU 参数。
    ///
    /// material 中的 texture path 保留 importer 原始表达，不在后台线程解析相对路径。
    /// 这样路径归一化、texture handle 分配和状态表更新仍统一收敛在 `AssetHub`。
    fn copy_material(&self, material_index: u32) -> Result<RawMaterialData, String> {
        let mut mat = truvixx::TruvixxMat::default();
        let res = unsafe { truvixx::truvixx_material_get(self.handle(), material_index, &mut mat as *mut _) };
        if res != truvixx::ResType_ResTypeSuccess {
            return Err(format!("failed to get material {}", material_index));
        }

        let diffuse_map = Self::read_fixed_c_string(&mat.diffuse_map);
        let normal_map = Self::read_fixed_c_string(&mat.normal_map);
        let name = Self::read_fixed_c_string(&mat.name);

        // texture 路径保持为导入器原始表达，稍后由 AssetHub 根据 scene 路径统一解析。
        Ok(RawMaterialData {
            base_color: Self::truvixx_float4_to_vec4(mat.base_color),
            emissive: Self::truvixx_float4_to_vec4(mat.emissive),
            metallic: mat.metallic,
            roughness: mat.roughness,
            opaque: mat.opacity,
            diffuse_texture_path: (!diffuse_map.is_empty()).then(|| PathBuf::from(diffuse_map)),
            normal_texture_path: (!normal_map.is_empty()).then(|| PathBuf::from(normal_map)),
            name: if name.is_empty() { format!("material-{}", material_index) } else { name },
        })
    }

    /// 复制一个 Assimp node 对应的 prefab instance 记录。
    ///
    /// 一个 node 可能引用多个 mesh，因此这里会拆成多条 `RawSceneInstanceData`。
    /// 返回值仍使用导入源内部的 mesh/material index，避免后台 FFI 任务直接分配
    /// `AssetMeshHandle` 或 `AssetMaterialHandle`。
    fn copy_instance(&self, instance_index: u32) -> Result<Vec<RawSceneInstanceData>, String> {
        let mut instance = truvixx::TruvixxInstance::default();
        let res = unsafe { truvixx::truvixx_instance_get(self.handle(), instance_index, &mut instance as *mut _) };
        if res != truvixx::ResType_ResTypeSuccess {
            return Err(format!("failed to get instance {}", instance_index));
        }

        if instance.mesh_count == 0 {
            return Ok(Vec::new());
        }

        let mesh_count = instance.mesh_count as usize;
        // C API 要求调用方提供至少 mesh_count 长度的输出数组。数组只作为 FFI
        // 临时写入缓冲，成功后立即被 Rust iterator 消费。
        let mut mesh_indices = vec![0_u32; mesh_count];
        let mut material_indices = vec![0_u32; mesh_count];

        let res = unsafe {
            truvixx::truvixx_instance_get_refs(
                self.handle(),
                instance_index,
                mesh_indices.as_mut_ptr(),
                material_indices.as_mut_ptr(),
            )
        };
        if res != truvixx::ResType_ResTypeSuccess {
            return Err(format!("failed to get instance {} refs", instance_index));
        }

        let instance_name = Self::read_fixed_c_string(&instance.name);
        let transform = Self::truvixx_float4x4_to_mat4(instance.world_transform);

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

    /// 读取 C API 固定长度 char buffer。
    ///
    /// C++ `safe_strcpy` 保证这些 buffer 以 NUL 结尾；本函数依赖这个 ABI 契约。
    /// 返回值会立即变成 owned String，调用方不会持有 C buffer 的借用。
    fn read_fixed_c_string(buffer: &[c_char]) -> String {
        // C API 用固定长度 char 数组返回字符串，并由 safe_strcpy 保证 NUL 结尾；
        // 这里立刻复制为 Rust String，不保留 C buffer 借用。
        unsafe { CStr::from_ptr(buffer.as_ptr()) }.to_string_lossy().into_owned()
    }

    /// TruvixxFloat* 是 C ABI 的紧凑 float 数组；C++ static_assert 与 bindgen
    /// layout assert 保证大小和对齐。Rust 侧不依赖 glam 的内存布局，只显式取值构造。
    ///
    /// 这些转换函数集中在 reader 内部，是为了让所有 `unsafe { value.v }` 的 union 字段
    /// 读取都有同一个注释边界；调用方只看到普通 `glam` 值。
    fn truvixx_float2_to_vec2(value: truvixx::TruvixxFloat2) -> glam::Vec2 {
        let v = unsafe { value.v };
        glam::Vec2::new(v[0], v[1])
    }

    /// 将 C ABI 的三维 float 向量显式转换为 `glam::Vec3`。
    ///
    /// 不使用 `transmute`，也不把 `TruvixxFloat3` slice 伪装成 `glam::Vec3` slice；
    /// 这样即使 `glam` 内部表示未来变化，FFI 读取契约也仍然清晰。
    fn truvixx_float3_to_vec3(value: truvixx::TruvixxFloat3) -> glam::Vec3 {
        let v = unsafe { value.v };
        glam::Vec3::new(v[0], v[1], v[2])
    }

    /// 将 C ABI 的四维 float 向量显式转换为 `glam::Vec4`。
    ///
    /// material base color / emissive 都走这里，确保颜色通道顺序由 `TruvixxFloat4::v`
    /// 的 ABI 顺序表达，而不是由 Rust 结构体布局推断。
    fn truvixx_float4_to_vec4(value: truvixx::TruvixxFloat4) -> glam::Vec4 {
        let v = unsafe { value.v };
        glam::Vec4::new(v[0], v[1], v[2], v[3])
    }

    /// 将 C ABI 的 4x4 矩阵显式转换为 `glam::Mat4`。
    ///
    /// `TruvixxFloat4x4::m` 按列连续存储：`col0, col1, col2, col3`。这里使用
    /// `from_cols_array` 保持列主序语义，避免把矩阵 layout 假设藏在 transmute 里。
    fn truvixx_float4x4_to_mat4(value: truvixx::TruvixxFloat4x4) -> glam::Mat4 {
        // TruvixxFloat4x4 的 m 数组按 col0..col3 排列，和 glam::Mat4::from_cols_array
        // 的列主序输入一致，因此不需要 transmute。
        let m = unsafe { value.m };
        glam::Mat4::from_cols_array(&m)
    }
}
