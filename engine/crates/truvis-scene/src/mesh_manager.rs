use std::collections::VecDeque;

use ash::vk;
use slotmap::SlotMap;

use truvis_gfx::raytracing::acceleration::GfxAcceleration;
use truvis_gfx::resources::special_buffers::index_buffer::GfxIndex32Buffer;
use truvis_gfx::resources::vertex_layout::soa_3d::VertexLayoutSoA3D;
use truvis_render_interface::geometry::RtGeometry;

use crate::guid_new_type::ManagedMeshHandle;

/// Mesh 在 MeshManager 中的状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeshStatus {
    /// 已注册，等待 GPU 上传和 BLAS 构建
    Pending,
    /// GPU 资源就绪，可用于渲染
    Ready,
}

/// Mesh 注册时的输入数据（CPU 侧，SoA 布局）
pub struct MeshInputData {
    pub positions: Vec<glam::Vec3>,
    pub normals: Vec<glam::Vec3>,
    pub tangents: Vec<glam::Vec3>,
    pub uvs: Vec<glam::Vec2>,
    pub indices: Vec<u32>,
    pub name: String,
}

/// MeshManager 内部维护的 mesh 条目
struct ManagedMesh {
    status: MeshStatus,

    /// 注册时暂存的输入数据，upload 后置为 None
    input: Option<MeshInputData>,

    /// GPU 侧几何体（vertex + index buffer）
    geometry: Option<RtGeometry>,

    /// BLAS 加速结构
    blas: Option<GfxAcceleration>,
    blas_device_address: Option<vk::DeviceAddress>,

    name: String,
}

/// 异步 Mesh 管理器
///
/// 管理 mesh 的注册、GPU 上传和 BLAS 构建。作为旁路模块存在，不影响现有 SceneManager / GpuScene 逻辑。
///
/// # 生命周期
/// 1. `register()` 接收 CPU 顶点数据，返回 Pending 状态的 handle
/// 2. `update()` 处理 pending 队列：创建 GPU buffer、上传顶点/索引数据、构建 BLAS
/// 3. 外部通过 `is_ready()` / `get_blas_device_address()` 查询状态，用于 TLAS 构建
///
/// # TLAS
/// MeshManager 仅管理 BLAS。TLAS 由外部根据 instance transform 信息构建。
pub struct MeshManager {
    meshes: SlotMap<ManagedMeshHandle, ManagedMesh>,

    /// 等待处理的 mesh handle 队列（FIFO）
    pending: VecDeque<ManagedMeshHandle>,
}

// new & destroy
impl MeshManager {
    pub fn new() -> Self {
        Self {
            meshes: SlotMap::with_key(),
            pending: VecDeque::new(),
        }
    }

    pub fn destroy(self) {}
}

impl Drop for MeshManager {
    fn drop(&mut self) {
        log::info!("Dropping MeshManager");
    }
}

// 注册 / 移除
impl MeshManager {
    /// 注册新 mesh，存储 CPU 数据并返回 Pending 状态的 handle
    ///
    /// GPU 上传和 BLAS 构建延迟到 `update()` 中执行。
    pub fn register(&mut self, input: MeshInputData) -> ManagedMeshHandle {
        let name = input.name.clone();
        let handle = self.meshes.insert(ManagedMesh {
            status: MeshStatus::Pending,
            input: Some(input),
            geometry: None,
            blas: None,
            blas_device_address: None,
            name: name.clone(),
        });
        self.pending.push_back(handle);

        log::debug!("MeshManager: register handle={:?} name={}", handle, name);
        handle
    }

    /// 移除 mesh，释放 GPU 资源
    ///
    /// 注意：当前版本直接销毁 GPU 资源。如果 mesh 正在被渲染管线引用，
    /// 调用方需确保相关 GPU 命令已完成。后续集成时可补充 FIF 延迟回收。
    pub fn unregister(&mut self, handle: ManagedMeshHandle) {
        if let Some(mesh) = self.meshes.remove(handle) {
            // 从 pending 队列移除（如果还在排队）
            self.pending.retain(|&h| h != handle);
            log::debug!("MeshManager: unregister handle={:?} name={}", handle, mesh.name);
        }
    }
}

// 帧更新
impl MeshManager {
    /// 处理 pending 队列：上传顶点/索引数据到 GPU 并构建 BLAS
    ///
    /// `max_per_frame` 限制每次调用处理的 mesh 数量，`None` 表示全部处理。
    pub fn update(&mut self, max_per_frame: Option<usize>) {
        let count = match max_per_frame {
            Some(max) => max.min(self.pending.len()),
            None => self.pending.len(),
        };

        for _ in 0..count {
            let handle = match self.pending.pop_front() {
                Some(h) => h,
                None => break,
            };

            let mesh = match self.meshes.get_mut(handle) {
                Some(m) => m,
                // handle 已被 unregister，跳过
                None => continue,
            };

            let input = match mesh.input.take() {
                Some(d) => d,
                None => continue,
            };

            Self::build_mesh(mesh, input);
        }
    }

    fn build_mesh(mesh: &mut ManagedMesh, input: MeshInputData) {
        let vertex_buffer = VertexLayoutSoA3D::create_vertex_buffer(
            &input.positions,
            &input.normals,
            &input.tangents,
            &input.uvs,
            format!("{}-vertex", mesh.name),
        );
        let index_buffer = GfxIndex32Buffer::new_with_data(&input.indices, format!("{}-index", mesh.name));

        let geometry = RtGeometry {
            vertex_buffer,
            index_buffer,
        };

        let blas_infos = [geometry.get_blas_geometry_info()];
        let blas = GfxAcceleration::build_blas_sync(
            &blas_infos,
            vk::BuildAccelerationStructureFlagsKHR::empty(),
            format!("{}-blas", mesh.name),
        );

        mesh.blas_device_address = Some(blas.device_address());
        mesh.blas = Some(blas);
        mesh.geometry = Some(geometry);
        mesh.status = MeshStatus::Ready;

        log::debug!("MeshManager: mesh '{}' is now Ready", mesh.name);
    }
}

// 查询
impl MeshManager {
    /// mesh 的 GPU 资源是否就绪
    #[inline]
    pub fn is_ready(&self, handle: ManagedMeshHandle) -> bool {
        self.meshes.get(handle).is_some_and(|m| m.status == MeshStatus::Ready)
    }

    /// 查询 mesh 的详细状态，handle 无效时返回 None
    #[inline]
    pub fn get_status(&self, handle: ManagedMeshHandle) -> Option<MeshStatus> {
        self.meshes.get(handle).map(|m| m.status)
    }

    /// 获取 BLAS device address（TLAS 构建时使用）
    ///
    /// mesh 未 Ready 时返回 None。
    #[inline]
    pub fn get_blas_device_address(&self, handle: ManagedMeshHandle) -> Option<vk::DeviceAddress> {
        self.meshes.get(handle).and_then(|m| m.blas_device_address)
    }

    /// 获取 mesh 的几何体引用（vertex/index buffer）
    #[inline]
    pub fn get_geometry(&self, handle: ManagedMeshHandle) -> Option<&RtGeometry> {
        self.meshes.get(handle).and_then(|m| m.geometry.as_ref())
    }

    /// 获取 mesh 名称
    #[inline]
    pub fn get_name(&self, handle: ManagedMeshHandle) -> Option<&str> {
        self.meshes.get(handle).map(|m| m.name.as_str())
    }

    /// 已注册的 mesh 总数（含 Pending 和 Ready）
    #[inline]
    pub fn mesh_count(&self) -> usize {
        self.meshes.len()
    }

    /// 当前 pending 队列中等待处理的 mesh 数量
    #[inline]
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// 遍历所有已 Ready 的 mesh（handle + BLAS device address）
    ///
    /// 用于外部构建 TLAS 时收集所有 BLAS 引用。
    pub fn ready_meshes(&self) -> impl Iterator<Item = (ManagedMeshHandle, vk::DeviceAddress)> + '_ {
        self.meshes.iter().filter_map(|(handle, mesh)| {
            mesh.blas_device_address.filter(|_| mesh.status == MeshStatus::Ready).map(|addr| (handle, addr))
        })
    }
}
