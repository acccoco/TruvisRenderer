//! CPU 侧 world 聚合层。
//!
//! `World` 是 update 阶段和 render runtime prepare 阶段之间的 CPU 数据入口，聚合
//! runtime scene 状态与 `truvis-asset` 的一次性 loader service。它不拥有 Vulkan、swapchain、
//! GPU buffer/image、frame state 或 shader binding 资源；这些对象由 render-side runtime 管理。

use std::path::PathBuf;

use truvis_asset::asset_hub::AssetHub;
use truvis_asset::handle::{LoadStatus, MeshData};
use truvis_shader_binding::gpu;

pub mod components;
mod edit_error;
pub mod guid_new_type;
pub mod procedural_mesh;
mod render_sync;
mod scene_asset_ingestor;
mod scene_store;

use crate::components::instance::Instance;
use crate::components::material::SceneMaterialData;
pub use crate::edit_error::{SceneEditError, SceneHandleKind, WorldEditError};
use crate::guid_new_type::{
    InstanceHandle, LightHandle, SceneMaterialHandle, SceneMeshHandle, SceneModelImportHandle, SceneTextureHandle,
};
pub use crate::render_sync::{
    FailedTextureLoad, PendingMeshUpload, PendingTextureUpload, SceneAssetSyncOutput, WorldRenderSync,
};
use crate::scene_asset_ingestor::SceneAssetIngestor;
use crate::scene_store::SceneStore;
pub use crate::scene_store::{
    SceneChanges, SceneInstanceChange, SceneInstanceChangeKind, SceneMaterialEmissiveView, SceneReadView, SceneSkyState,
};

/// CPU 侧场景状态的聚合容器。
///
/// 与 GPU-facing 状态物理分离，建立 CPU/GPU 数据的所有权边界。App /
/// Plugin 在 update 阶段通过这里修改 CPU state；`RenderRuntime::prepare` 再读取这些数据，
/// 同步到 render-side manager、bridge、`RenderWorld` 和 shader-visible bindings。
pub struct World {
    /// runtime scene 语义数据，包括 live instance 和 light。
    ///
    /// 这里的 handle 是 CPU runtime 身份，不是 GPU slot；渲染运行时负责把它们同步到
    /// GPU-visible scene 数据。
    scene: SceneStore,
    /// 一次性 CPU asset loader service。
    ///
    /// `AssetHub` 负责 loader task 和 CPU ready 数据汇聚；它的内部 handle 不作为
    /// scene 或 render-world 的长期身份暴露。
    assets: AssetHub,
    /// scene-facing asset ingest 状态。
    ///
    /// 该对象负责把 App 看到的 scene import 请求映射到内部 asset loader 状态，避免
    /// `ModelLoadHandle` 等 loader 细节扩散到 App 层。
    scene_assets: SceneAssetIngestor,
}

// 创建与销毁
impl World {
    /// 创建 CPU world，并在内部初始化 scene store、asset loader service 和 ingest pipeline。
    pub fn new() -> Self {
        Self {
            scene: SceneStore::new(),
            assets: AssetHub::new(),
            scene_assets: SceneAssetIngestor::new(),
        }
    }

    /// 在 render runtime 销毁阶段先清空 CPU scene。
    ///
    /// 该方法服务现有销毁顺序：scene runtime 语义先停止，render-side material / texture /
    /// mesh / GPU scene 缓存随后释放，最后再消费并销毁整个 `World`。
    pub fn destroy_scene_mut(&mut self) {
        self.scene.destroy_mut();
    }

    /// 消耗 `World`，释放其 CPU asset owner。
    ///
    /// GPU 资源不属于 `World`，因此这里不会访问任何 Vulkan/VMA 对象；调用方必须在自己的
    /// render-side owner 中按依赖顺序显式释放 GPU 资源。
    pub fn destroy(self) {
        self.assets.destroy();
    }
}

// Render runtime-facing 同步接口
impl World {
    /// 同步 CPU scene / asset 状态，生成 render prepare 消费的本帧同步包。
    ///
    /// 这是 render runtime 在 update 之后、prepare 之初接触 asset event 的唯一入口。
    /// `World` 只负责把后台 loader 结果收敛回调用线程并翻译成 scene handle；texture /
    /// mesh / material 的 GPU 上传仍由 render-side manager 负责。
    pub fn sync_for_render(&mut self) -> WorldRenderSync {
        let events = self.assets.update();
        let asset_uploads = self.scene_assets.ingest_asset_events(&mut self.assets, &mut self.scene, events);
        let scene_changes = self.scene.drain_changes();
        WorldRenderSync {
            scene_changes,
            asset_uploads,
        }
    }

    /// 返回 CPU scene 的只读视图。
    ///
    /// 当前 render-side `RenderInstanceManager` 仍需要读取 `SceneStore` 的 live instance / light
    /// 快照。该 accessor 不暴露 `SceneStore` owner，避免 render runtime 或 App 修改 CPU scene 语义。
    pub fn scene_view(&self) -> SceneReadView<'_> {
        SceneReadView::new(&self.scene)
    }
}

// App 侧 facade
impl World {
    /// 请求导入 model / prefab。
    ///
    /// 返回值是 scene-facing import handle；调用方不需要知道 `AssetHub` 的内部 load handle。
    pub fn request_model_import(&mut self, path: PathBuf) -> SceneModelImportHandle {
        self.scene_assets.request_model_import(&mut self.assets, path)
    }

    /// 注册一个 file texture 并返回 scene-facing texture handle。
    pub fn register_texture(&mut self, path: PathBuf) -> Result<SceneTextureHandle, WorldEditError> {
        let canonical_path =
            std::fs::canonicalize(&path).map_err(|err| WorldEditError::FilesystemCanonicalizeFailed {
                path: path.clone(),
                error: err.to_string(),
            })?;
        Ok(self.scene_assets.register_texture_canonical(&mut self.assets, &mut self.scene, canonical_path))
    }

    /// 查询 model import 的 CPU 加载状态。
    ///
    /// App 只用它显示或驱动 UI，不直接读取 `AssetHub` 的 loader state。
    pub fn model_import_status(&self, handle: SceneModelImportHandle) -> LoadStatus {
        self.scene_assets.model_import_status(handle)
    }

    /// 查询 model import 的失败文本。
    pub fn model_import_error(&self, handle: SceneModelImportHandle) -> Option<&str> {
        self.scene_assets.model_import_error(handle)
    }

    /// 注册已经在 CPU 内存中的 mesh 数据。
    ///
    /// `SceneAssetIngestor` 会把内部 loader 事件转换为 `SceneMeshHandle` 标记的短期
    /// mesh upload payload，render-side manager 不接触 asset mesh handle。
    pub fn register_mesh(&mut self, data: MeshData) -> Result<SceneMeshHandle, WorldEditError> {
        Ok(self.scene_assets.register_mesh(&mut self.scene, data))
    }

    /// 注册已经在 CPU 内存中的 material 参数。
    ///
    /// `SceneMaterialData` 内部使用 scene texture handle；render-side material manager
    /// 后续通过 `SceneChanges.changed_materials` 从 `SceneStore` 读取权威参数。
    pub fn register_material(&mut self, data: SceneMaterialData) -> Result<SceneMaterialHandle, WorldEditError> {
        self.scene_assets.register_material(&mut self.scene, data).map_err(Into::into)
    }

    /// 更新 CPU material 参数，并记录 scene change。
    pub fn update_material(
        &mut self,
        handle: SceneMaterialHandle,
        data: SceneMaterialData,
    ) -> Result<(), WorldEditError> {
        self.scene.update_material(handle, data)?;
        Ok(())
    }

    /// 移除未被 instance 引用的 CPU material。
    pub fn remove_material(&mut self, handle: SceneMaterialHandle) -> Result<(), WorldEditError> {
        self.scene.remove_material(handle).map_err(Into::into)
    }

    /// 移除未被 material 引用的 CPU texture。
    pub fn remove_texture(&mut self, handle: SceneTextureHandle) -> Result<(), WorldEditError> {
        self.scene.remove_texture(handle).map_err(Into::into)
    }

    /// 移除未被 instance 引用的 CPU mesh。
    pub fn remove_mesh(&mut self, handle: SceneMeshHandle) -> Result<(), WorldEditError> {
        self.scene.remove_mesh(handle).map_err(Into::into)
    }

    /// 更新 CPU sky 引用的 scene texture。
    pub fn update_sky_texture(&mut self, texture: Option<SceneTextureHandle>) -> Result<(), WorldEditError> {
        self.scene.update_sky_texture(texture).map_err(Into::into)
    }

    /// 更新 CPU sky 是否启用。
    pub fn update_sky_enabled(&mut self, enabled: bool) {
        self.scene.update_sky_enabled(enabled);
    }

    /// 更新 CPU sky 亮度语义参数。
    pub fn update_sky_intensity(&mut self, intensity: f32) {
        self.scene.update_sky_intensity(intensity);
    }

    /// 查询当前 CPU material 参数。
    ///
    /// 这是 App/debug UI 的只读 facade；返回数据属于 CPU scene 参数，不表示 GPU material slot
    /// 已经 ready，也不暴露 loader owner 给调用方。
    pub fn material_data(&self, handle: SceneMaterialHandle) -> Option<&SceneMaterialData> {
        self.scene.material_data(handle)
    }

    /// 注册一个 CPU runtime instance。
    pub fn register_instance(&mut self, instance: Instance) -> Result<InstanceHandle, WorldEditError> {
        self.scene.register_instance(instance).map_err(Into::into)
    }

    /// 更新一个 CPU runtime instance 的 material 绑定。
    pub fn update_instance_materials(
        &mut self,
        handle: InstanceHandle,
        materials: Vec<SceneMaterialHandle>,
    ) -> Result<(), WorldEditError> {
        self.scene.update_instance_materials(handle, materials).map_err(Into::into)
    }

    /// 更新一个 CPU runtime instance 的 world transform。
    pub fn update_instance_transform(
        &mut self,
        handle: InstanceHandle,
        transform: glam::Mat4,
    ) -> Result<(), WorldEditError> {
        self.scene.update_instance_transform(handle, transform).map_err(Into::into)
    }

    /// 移除一个 CPU runtime instance。
    pub fn remove_instance(&mut self, handle: InstanceHandle) -> Result<(), WorldEditError> {
        self.scene.remove_instance(handle).map_err(Into::into)
    }

    /// 注册 point light。
    pub fn register_point_light(&mut self, light: gpu::light::PointLight) -> LightHandle {
        self.scene.register_point_light(light)
    }

    /// 注册 spot light。
    pub fn register_spot_light(&mut self, light: gpu::light::SpotLight) -> LightHandle {
        self.scene.register_spot_light(light)
    }

    /// 注册 area light。
    pub fn register_area_light(&mut self, light: gpu::light::AreaLight) -> LightHandle {
        self.scene.register_area_light(light)
    }
}
