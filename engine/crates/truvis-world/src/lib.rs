use truvis_asset::asset_hub::AssetHub;
use truvis_scene::scene_manager::SceneManager;

/// CPU 侧场景状态的聚合容器。
///
/// 与 GPU 渲染状态（`RenderWorld`）物理分离，
/// 建立 CPU/GPU 数据的所有权边界。
pub struct World {
    pub scene_manager: SceneManager,
    pub asset_hub: AssetHub,
}
