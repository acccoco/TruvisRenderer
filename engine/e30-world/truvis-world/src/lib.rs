//! CPU 侧 world 聚合层。
//!
//! `GameWorld` 是 update 阶段和 render runtime prepare 阶段之间的 CPU 数据入口，聚合
//! runtime scene 状态与 `AssetSystem` 的一次性 loader service。它不拥有 Vulkan、swapchain、
//! GPU buffer/image、frame state 或 shader binding 资源；这些对象由 render-side runtime 管理。

use std::path::PathBuf;

use truvis_asset::handle::{LoadStatus, MeshData, TextureColorSpace};
use truvis_shader_binding::gpu;

mod asset_system;
pub mod components;
mod edit_error;
pub mod guid_new_type;
mod light_parameters;
mod light_target;
pub mod procedural_mesh;
mod scene_store;

pub use crate::asset_system::{AssetSource, AssetSystem, GeneratedSourceKey, SceneData, SceneObjectData, TextureRecord, TextureState};
use crate::components::instance::Instance;
use crate::components::material::MaterialData;
pub use crate::edit_error::{SceneEditError, SceneHandleKind, WorldEditError};
use crate::guid_new_type::{
    LightHandle, MaterialAssetHandle, MeshAssetHandle, MeshInstanceHandle, SceneImportHandle, TextureAssetHandle,
};
pub use crate::light_target::LightTarget;
use crate::scene_store::SceneStore;
pub use crate::scene_store::{SceneReadView, SceneSkyState};

pub use crate::light_parameters::{AreaLightShape, LightPatch};

/// CPU 侧场景状态的聚合容器。
///
/// 与 GPU-facing 状态物理分离，建立 CPU/GPU 数据的所有权边界。Renderer 及其具体
/// 子系统在 update 阶段通过这里修改 CPU state；`RenderRuntime::prepare` 再读取这些数据，
/// 同步到 render-side manager、bridge、`RenderWorld` 和 shader-visible bindings。
pub struct GameWorld {
    /// runtime scene 语义数据，包括 live instance 和 light。
    ///
    /// 这里的 handle 是 CPU runtime 身份，不是 GPU slot；渲染运行时负责把它们同步到
    /// GPU-visible scene 数据。
    scene: SceneStore,
    /// CPU 资源唯一 owner；不创建 GPU 对象，也不保存 instance 组合关系。
    resources: AssetSystem,
}

// 创建与销毁
impl GameWorld {
    /// 创建 CPU world，并在内部初始化 scene store 和 asset system。
    pub fn new() -> Self {
        Self {
            scene: SceneStore::new(),
            resources: AssetSystem::new(),
        }
    }

    /// 在 render runtime 销毁阶段先清空 CPU scene。
    ///
    /// 该方法服务现有销毁顺序：scene runtime 语义先停止，render-side scene 缓存随后释放，
    /// 最后再消费并销毁整个 `GameWorld`。
    pub fn destroy_scene_mut(&mut self) {
        self.scene.destroy_mut();
    }

    /// 消耗 `GameWorld`，释放其 CPU asset owner。
    ///
    /// GPU 资源不属于 `GameWorld`，因此这里不会访问任何 Vulkan/VMA 对象；调用方必须在自己的
    /// render-side owner 中按依赖顺序显式释放 GPU 资源。
    pub fn destroy(self) {
        self.resources.destroy();
    }
}

// Render runtime-facing 同步接口
impl GameWorld {
    /// 将后台 loader 完成结果收敛到 CPU 资源表与场景。
    ///
    /// 这是 render runtime 在 update 之后、prepare 之初收敛 loader completion 的唯一入口。
    /// `GameWorld` 只负责把后台 loader 结果收敛回调用线程并翻译成 CPU resource handle；texture /
    /// mesh / material 的 GPU 上传仍由 render-side manager 负责。
    pub fn poll_asset_loads(&mut self) {
        self.resources.poll_asset_loads();
    }

    /// 返回 CPU scene 的只读视图。
    ///
    /// 当前 render-side `RenderInstanceTable` 仍需要读取 `SceneStore` 的 live instance / light
    /// 快照。该 accessor 不暴露 `SceneStore` owner，避免 render runtime 或 Renderer 绕过 facade 修改 CPU scene 语义。
    pub fn scene_view(&self) -> SceneReadView<'_> {
        SceneReadView::new(&self.scene, &self.resources.store)
    }
}

// Renderer 侧 facade
impl GameWorld {
    /// 请求导入 scene / prefab 文件。
    ///
    /// 返回值是 CPU world import handle；调用方不需要知道 `AssetLoadService` 的内部 load handle。
    pub fn import_scene(&mut self, path: PathBuf) -> SceneImportHandle {
        self.resources.import_scene(path)
    }

    /// 注册一个 file texture 并返回 CPU world texture handle。
    pub fn import_texture(
        &mut self,
        path: PathBuf,
        color_space: TextureColorSpace,
    ) -> Result<TextureAssetHandle, WorldEditError> {
        let original_path = path.clone();
        let (texture, is_new) = self.resources.import_texture_file(path, color_space).map_err(|error| {
            WorldEditError::FilesystemCanonicalizeFailed {
                path: original_path,
                error,
            }
        })?;
        if is_new {
            self.scene.mark_resource_changed();
        }
        Ok(texture)
    }

    /// 注册 file texture 并立即把它设为 scene sky。
    ///
    /// 返回只表示 CPU scene 已接受请求，不等待文件解码、GPU image upload 或 sky
    /// distribution build。等待期间 render-side 保持 sky fallback；失败时也保持 fallback
    /// 并记录 loader 错误。旧 texture 不会自动删除，因为它仍可能被 material 引用。
    pub fn request_sky_texture_from_path(&mut self, path: PathBuf) -> Result<TextureAssetHandle, WorldEditError> {
        let texture = self.import_texture(path, TextureColorSpace::Linear)?;
        self.update_sky_texture(Some(texture))?;
        Ok(texture)
    }

    /// 查询 scene import 的 CPU 加载状态。
    ///
    /// Renderer 只用它显示或驱动 UI，不直接读取 `AssetLoadService` 的 loader state。
    pub fn scene_import_status(&self, handle: SceneImportHandle) -> LoadStatus {
        self.resources.scene_import_status(handle)
    }

    /// 查询 scene import 的失败文本。
    pub fn scene_import_error(&self, handle: SceneImportHandle) -> Option<&str> {
        self.resources.scene_import_error(handle)
    }

    /// 查询已经完成资源身份解析的 scene 数据；不会创建 `SceneStore` instance。
    pub fn scene_data(&self, handle: SceneImportHandle) -> Option<&SceneData> {
        self.resources.scene_data(handle)
    }

    /// 注册已经在 CPU 内存中的 mesh 数据。
    ///
    /// CPU registry 接管不可变内容；渲染资源层在下次同步时借用数据创建 GPU mesh。
    pub fn import_mesh(&mut self, data: MeshData) -> Result<MeshAssetHandle, WorldEditError> {
        let mesh = self.resources.register_mesh(data).map_err(WorldEditError::from)?;
        self.scene.mark_resource_changed();
        Ok(mesh)
    }

    /// 注册已经在 CPU 内存中的 material 参数。
    ///
    /// `MaterialData` 内部使用 `TextureAssetHandle`；render-side material manager
    /// 在 prepare 阶段通过 `SceneReadView` 对账 CPU 权威参数。
    pub fn register_material(&mut self, data: MaterialData) -> Result<MaterialAssetHandle, WorldEditError> {
        let material = self.resources.register_material(data).map_err(WorldEditError::from)?;
        self.scene.mark_resource_changed();
        Ok(material)
    }

    /// 更新 CPU material 参数；实际变化才推进 source revision。
    pub fn update_material(&mut self, handle: MaterialAssetHandle, data: MaterialData) -> Result<(), WorldEditError> {
        self.scene.validate_material_update(&self.resources.store, handle, &data)?;
        if self.resources.update_material(handle, data)? {
            self.scene.mark_resource_changed();
        }
        Ok(())
    }

    /// 移除未被 instance 引用的 CPU material。
    pub fn remove_material(&mut self, handle: MaterialAssetHandle) -> Result<(), WorldEditError> {
        self.resources
            .remove_material(handle, self.scene.instance_dependents_for_material(handle))
            .map_err(WorldEditError::from)?;
        self.scene.mark_resource_changed();
        Ok(())
    }

    /// 移除未被 material 引用的 CPU texture。
    pub fn remove_texture(&mut self, handle: TextureAssetHandle) -> Result<(), WorldEditError> {
        let dependent_count = self.resources.store.materials_using_texture(handle).count()
            + usize::from(self.scene.sky_uses_texture(handle));
        self.resources.remove_texture(handle, dependent_count).map_err(WorldEditError::from)?;
        self.scene.mark_resource_changed();
        Ok(())
    }

    /// 移除未被 instance 引用的 CPU mesh。
    pub fn remove_mesh(&mut self, handle: MeshAssetHandle) -> Result<(), WorldEditError> {
        self.resources
            .remove_mesh(handle, self.scene.instance_dependents_for_mesh(handle))
            .map_err(WorldEditError::from)?;
        self.scene.mark_resource_changed();
        Ok(())
    }

    /// 更新 CPU sky 引用的 scene texture。
    pub fn update_sky_texture(&mut self, texture: Option<TextureAssetHandle>) -> Result<(), WorldEditError> {
        self.scene.update_sky_texture(texture, &self.resources.store).map_err(Into::into)
    }

    /// 原子修改环境参数，失败不推进版本。
    pub fn update_sky_parameters(
        &mut self,
        enabled: Option<bool>,
        brightness: Option<f32>,
    ) -> Result<(), WorldEditError> {
        self.scene.update_sky_parameters(enabled, brightness).map_err(Into::into)
    }

    /// 更新 CPU sky 是否启用。
    pub fn update_sky_enabled(&mut self, enabled: bool) {
        self.scene.update_sky_parameters(Some(enabled), None).expect("unchanged brightness is valid");
    }

    /// 查询当前 CPU material 参数。
    ///
    /// 这是 Renderer/debug UI 的只读 facade；返回数据属于 CPU scene 参数，不表示 GPU material slot
    /// 已经 ready，也不暴露 loader owner 给调用方。
    pub fn material_data(&self, handle: MaterialAssetHandle) -> Option<&MaterialData> {
        self.resources.store.material_data(handle)
    }

    /// 查询引用指定 material 的 live instance。
    pub fn instances_using_material(&self, handle: MaterialAssetHandle) -> Vec<MeshInstanceHandle> {
        self.scene_view().instances_using_material(handle).collect()
    }

    /// 查询引用指定 mesh 的 live instance。
    pub fn instances_using_mesh(&self, handle: MeshAssetHandle) -> Vec<MeshInstanceHandle> {
        self.scene_view().instances_using_mesh(handle).collect()
    }

    /// 查询引用指定 texture 的 material。
    pub fn materials_using_texture(&self, handle: TextureAssetHandle) -> Vec<MaterialAssetHandle> {
        self.scene_view().materials_using_texture(handle).collect()
    }

    /// 注册一个 CPU runtime instance。
    pub fn create_mesh_instance(&mut self, instance: Instance) -> Result<MeshInstanceHandle, WorldEditError> {
        self.scene.register_instance(&self.resources.store, instance).map_err(Into::into)
    }

    /// 更新一个 CPU runtime instance 的 material 绑定。
    pub fn update_instance_materials(
        &mut self,
        handle: MeshInstanceHandle,
        materials: Vec<MaterialAssetHandle>,
    ) -> Result<(), WorldEditError> {
        self.scene.update_instance_materials(&self.resources.store, handle, materials).map_err(Into::into)
    }

    /// 更新一个 CPU runtime instance 的 world transform。
    pub fn update_instance_transform(
        &mut self,
        handle: MeshInstanceHandle,
        transform: glam::Mat4,
    ) -> Result<(), WorldEditError> {
        self.scene.update_instance_transform(handle, transform).map_err(Into::into)
    }

    /// 移除一个 CPU runtime instance。
    pub fn remove_mesh_instance(&mut self, handle: MeshInstanceHandle) -> Result<(), WorldEditError> {
        self.scene.remove_instance(handle).map_err(Into::into)
    }

    /// 修改灯光世界位置，保留形状参数并推进场景和灯光版本。
    pub fn update_light_position(&mut self, target: LightTarget, position: glam::Vec3) -> Result<(), WorldEditError> {
        self.update_light(
            target,
            LightPatch {
                position: Some(position),
                ..Default::default()
            },
        )?;
        Ok(())
    }

    /// 按字段合并最新灯光参数，校验成功后提交。
    pub fn update_light(&mut self, target: LightTarget, patch: LightPatch) -> Result<(), WorldEditError> {
        self.scene.update_light(target, patch).map_err(Into::into)
    }

    /// 注册 point light。
    pub fn register_point_light(&mut self, light: gpu::engine::light::PointLight) -> LightHandle {
        self.scene.register_point_light(light)
    }

    /// 注册 spot light。
    pub fn register_spot_light(&mut self, light: gpu::engine::light::SpotLight) -> LightHandle {
        self.scene.register_spot_light(light)
    }

    /// 注册 area light。
    pub fn register_area_light(&mut self, light: gpu::engine::light::AreaLight) -> LightHandle {
        self.scene.register_area_light(light)
    }
}
