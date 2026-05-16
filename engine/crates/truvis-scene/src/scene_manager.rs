use indexmap::IndexMap;
use slotmap::SlotMap;

use truvis_gfx::gfx::{GfxDeviceCtx, GfxResourceCtx};
use truvis_render_interface::render_data::{InstanceRenderData, MeshRenderData, RenderData};
use truvis_shader_binding::gpu;

use crate::components::instance::Instance;
use crate::components::material::Material;
use crate::components::mesh::Mesh;
use crate::guid_new_type::{InstanceHandle, LightHandle, MaterialHandle, MeshHandle};

/// CPU material handle 到稳定 GPU material slot 的解析接口。
///
/// 由 render-side material bridge 实现，SceneManager 只依赖 slot 结果，
/// 不接触 texture、bindless 或 GPU material buffer 的细节。
pub trait MaterialSlotResolver {
    fn resolve_material_slot(&self, handle: MaterialHandle) -> Option<u32>;
}

/// 在 CPU 侧管理场景数据
#[derive(Default)]
pub struct SceneManager {
    all_mats: SlotMap<MaterialHandle, Material>,
    all_instances: SlotMap<InstanceHandle, Instance>,
    all_meshes: SlotMap<MeshHandle, Mesh>,

    all_point_lights: SlotMap<LightHandle, gpu::PointLight>,
}
// 创建与初始化
impl SceneManager {
    pub fn new() -> Self {
        Self::default()
    }
}
// 访问器
impl SceneManager {
    #[inline]
    pub fn mat_map(&self) -> &SlotMap<MaterialHandle, Material> {
        &self.all_mats
    }
    #[inline]
    pub fn instance_map(&self) -> &SlotMap<InstanceHandle, Instance> {
        &self.all_instances
    }
    #[inline]
    pub fn mesh_map(&self) -> &SlotMap<MeshHandle, Mesh> {
        &self.all_meshes
    }
    #[inline]
    pub fn point_light_map(&self) -> &SlotMap<LightHandle, gpu::PointLight> {
        &self.all_point_lights
    }
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.all_instances.is_empty()
            && self.all_meshes.is_empty()
            && self.all_mats.is_empty()
            && self.all_point_lights.is_empty()
    }

    /// 构建完整的场景数据快照（SceneData2）
    ///
    /// 该方法会遍历所有场景数据，构建一个自包含的 SceneData2 结构，
    /// 使得 GpuScene 可以独立于 SceneManager 完成 GPU buffer 的构建和上传。
    ///
    /// # 参数
    /// - `material_slot_resolver`: 用于把 CPU material handle 解析为稳定 GPU slot
    ///
    /// # 返回
    /// 包含完整场景信息的 SceneData2 结构
    pub fn prepare_render_data<'a>(&'a self, material_slot_resolver: &dyn MaterialSlotResolver) -> RenderData<'a> {
        if self.is_empty() {
            return RenderData::empty();
        }

        // 1. 构建 mesh handle -> index 映射，以及 mesh 数据
        let mut mesh_handle_to_index: IndexMap<MeshHandle, usize> = IndexMap::new();
        let mut all_meshes: Vec<MeshRenderData<'a>> = Vec::with_capacity(self.all_meshes.len());
        let mut mesh_geometry_start_indices: Vec<usize> = Vec::with_capacity(self.all_meshes.len());
        let mut total_geometry_count: usize = 0;

        for (handle, mesh) in self.all_meshes.iter() {
            let index = all_meshes.len();
            mesh_handle_to_index.insert(handle, index);
            mesh_geometry_start_indices.push(total_geometry_count);
            total_geometry_count += mesh.geometries.len();

            all_meshes.push(MeshRenderData {
                geometries: &mesh.geometries,
                blas_device_address: mesh.blas_device_address,
                name: &mesh.name,
            });
        }

        // 2. 构建 instance 数据，材质引用写入稳定 GPU material slot
        let mut all_instances: Vec<InstanceRenderData> = Vec::with_capacity(self.all_instances.len());

        for (_handle, instance) in self.all_instances.iter() {
            let mesh_index = *mesh_handle_to_index.get(&instance.mesh).expect("Mesh not found for instance");
            let material_slots: Vec<u32> = instance
                .materials
                .iter()
                .map(|&mat_handle| {
                    material_slot_resolver
                        .resolve_material_slot(mat_handle)
                        .expect("Material slot not found for instance")
                })
                .collect();

            all_instances.push(InstanceRenderData {
                mesh_index,
                material_slots,
                transform: instance.transform,
            });
        }

        // 3. 构建点光源数据
        let all_point_lights: Vec<gpu::PointLight> = self.all_point_lights.iter().map(|(_, light)| *light).collect();

        RenderData {
            all_instances,
            all_meshes,
            all_point_lights,
            mesh_geometry_start_indices,
            total_geometry_count,
        }
    }
}
// 工具函数
impl SceneManager {
    #[inline]
    pub fn get_instance(&self, handle: InstanceHandle) -> Option<&Instance> {
        self.all_instances.get(handle)
    }

    #[inline]
    pub fn get_mesh(&self, handle: MeshHandle) -> Option<&Mesh> {
        self.all_meshes.get(handle)
    }

    #[inline]
    pub fn get_material(&self, handle: MaterialHandle) -> Option<&Material> {
        self.all_mats.get(handle)
    }

    /// 向场景中添加材质
    pub fn register_mat(&mut self, mat: Material) -> MaterialHandle {
        self.all_mats.insert(mat)
    }

    /// 向场景中添加 mesh
    pub fn register_mesh(&mut self, mesh: Mesh) -> MeshHandle {
        self.all_meshes.insert(mesh)
    }

    /// 向场景中添加 instance
    pub fn register_instance(&mut self, instance: Instance) -> InstanceHandle {
        self.all_instances.insert(instance)
    }

    /// 向场景中添加点光源
    pub fn register_point_light(&mut self, light: gpu::PointLight) -> LightHandle {
        self.all_point_lights.insert(light)
    }
}
impl Drop for SceneManager {
    fn drop(&mut self) {
        log::info!("SceneManager dropped.");
    }
}
// 销毁
impl SceneManager {
    pub fn destroy(mut self, resource_ctx: GfxResourceCtx<'_>, device_ctx: GfxDeviceCtx<'_>) {
        self.destroy_mut(resource_ctx, device_ctx);
    }
    pub fn destroy_mut(&mut self, resource_ctx: GfxResourceCtx<'_>, device_ctx: GfxDeviceCtx<'_>) {
        for (_, mesh) in self.all_meshes.iter_mut() {
            mesh.destroy_mut(resource_ctx, device_ctx);
        }
        self.all_mats.clear();
        self.all_instances.clear();
        self.all_meshes.clear();
        self.all_point_lights.clear();
    }
}
