use renderer_kit::camera::Camera;
use truvis_renderer::RendererClient;
use truvis_renderer::{SceneSelection, SelectionChange};
use truvis_scenes::{InitialScene, SceneInitializer};
use truvis_world::GameWorld;

/// standalone App 的 RenderThread Client；只拥有场景初始化状态，不创建 Editor 通信设施。
pub(crate) struct CornellAppClient {
    scene: SceneInitializer,
}

impl CornellAppClient {
    pub(crate) fn new(initial_scene: InitialScene) -> Self {
        Self {
            scene: SceneInitializer::new(initial_scene),
        }
    }
}

impl RendererClient for CornellAppClient {
    fn initialize(&mut self, world: &mut GameWorld, camera: &mut Camera) {
        self.scene.initialize(world, camera);
    }

    fn tick(&mut self, world: &mut GameWorld, _selection: Option<SceneSelection>) {
        self.scene.update(world);
    }

    /// 没有外部 Editor 消费通知；Renderer 自己仍正常维护 selection 和描边。
    fn on_selection_changed(&mut self, _selection: Option<SelectionChange>) {}

    /// 没有待关闭的 receiver 或 GPU 资源；CPU 资源继续由 GameWorld owner 回收。
    fn shutdown(&mut self) {}
}
