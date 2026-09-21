//! App 在 RenderThread 上提供的产品业务 Client 边界。

use renderer_kit::camera::Camera;
use truvis_render_runtime::selection::WorldSubmeshSelection;
use truvis_world::GameWorld;
use truvis_world::guid_new_type::MaterialAssetHandle;

/// 由 App 注入、但只在 RenderThread 生命周期阶段执行的产品业务 Client。
///
/// Client 可以在受控阶段借用 CPU `GameWorld`，负责场景组合和 Editor 业务请求；
/// RenderLoop 继续拥有 RenderRuntime；Renderer 负责 GPU 查询、RenderGraph 及具体渲染 subsystem。
pub trait RendererClient: Send {
    /// 注册启动场景和初始相机状态。
    fn initialize(&mut self, world: &mut GameWorld, camera: &mut Camera);

    /// 在每帧 `update` 阶段处理 CPU scene 业务，之后才进入 runtime prepare。
    fn tick(&mut self, world: &mut GameWorld, selection: Option<WorldSubmeshSelection>);

    /// `after_prepare` 的拾取结果或 update 中的选择清空触发产品通知。
    ///
    /// Editor selection DTO 还需要 material handle，因此这里同时传递命中的 material；
    /// 不让 Client 维护一份 instance/submesh 到 material 的缓存。
    fn on_selection_changed(&mut self, selection: Option<(WorldSubmeshSelection, MaterialAssetHandle)>);

    /// 在 Runtime root owner 销毁前关闭 Client 自己的请求 receiver。
    fn shutdown(&mut self);
}
