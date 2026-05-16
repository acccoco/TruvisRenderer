use truvis_asset::asset_hub::AssetHub;
use truvis_scene::guid_new_type::InstanceHandle;
use truvis_scene::scene_manager::SceneManager;

/// Assimp scene loader 的兼容入口。
///
/// Assimp CPU 导入已经迁移到 `AssetHub::load_scene()`。此类型只保留旧 API 形状，
/// 方便未迁移调用方请求 scene asset；如果 scene 已经 CPU ready，则立即 spawn。
#[deprecated(note = "Use AssetHub::load_scene() and SceneManager::spawn_scene_asset() instead")]
pub struct AssimpSceneLoader;

#[allow(deprecated)]
impl AssimpSceneLoader {
    /// 请求加载 scene，并在已有 ready 数据时 spawn runtime instances。
    ///
    /// 新路径是异步的；首次调用通常只会返回空 Vec，调用方应在后续 update 阶段检查
    /// `AssetHub::get_scene_status()` 并显式 spawn。
    #[deprecated(note = "Use AssetHub::load_scene() and SceneManager::spawn_scene_asset() instead")]
    pub fn load_scene(
        model_file: &std::path::Path,
        scene_manager: &mut SceneManager,
        asset_hub: &mut AssetHub,
    ) -> Vec<InstanceHandle> {
        let scene_handle = asset_hub.load_scene(model_file.to_path_buf());
        let Some(scene_data) = asset_hub.get_scene_data(scene_handle) else {
            log::warn!("AssimpSceneLoader compatibility path requested {:?}; scene data is not ready yet", model_file);
            return Vec::new();
        };

        scene_manager.spawn_scene_asset(scene_data)
    }
}
