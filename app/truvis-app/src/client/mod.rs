//! Truvis App 在 RenderThread 上运行的场景与 Editor 业务 Client。

mod desktop_command;
mod editor_controller;
mod scenes;

use renderer_kit::camera::Camera;
use truvis_editor_bridge::{EditorBridgeConfig, FrontendEndpoint, RendererEndpoint, create_editor_bridge};
use truvis_render_runtime::selection::WorldSubmeshSelection;
use truvis_renderer::RendererClient;
use truvis_world::GameWorld;
use truvis_world::guid_new_type::MaterialAssetHandle;

use self::desktop_command::DesktopCommandController;
use self::editor_controller::{EditorController, EditorControllerConfig};
use self::scenes::SceneInitializer;

pub(crate) use self::desktop_command::DesktopCommandSender;
pub(crate) use self::scenes::InitialScene;

/// App 主线程和 RenderThread 之间的一次性通信装配结果。
///
/// `frontend_editor` 和 sender 留在 Tauri 主线程；`client_ports` 只随 Renderer factory
/// 移入 RenderThread，避免 App 主线程持有 CPU scene 或请求 receiver。
pub(crate) struct TruvisAppWiring {
    pub(crate) frontend_editor: FrontendEndpoint,
    pub(crate) client_ports: TruvisAppClientPorts,
    pub(crate) desktop_command_sender: DesktopCommandSender,
}

impl TruvisAppWiring {
    pub(crate) fn new(config: EditorBridgeConfig) -> Self {
        let (frontend_editor, editor) = create_editor_bridge(config);
        let (desktop_command_sender, desktop_commands) = DesktopCommandController::create();
        Self {
            frontend_editor,
            client_ports: TruvisAppClientPorts {
                editor,
                desktop_commands,
            },
            desktop_command_sender,
        }
    }
}

/// 只包含 RenderThread 侧 receiver 的 Client 构造输入。
pub(crate) struct TruvisAppClientPorts {
    editor: RendererEndpoint,
    desktop_commands: DesktopCommandController,
}

/// 运行在 RenderThread 上的 Truvis 产品业务 Client。
pub(crate) struct TruvisAppClient {
    scene: SceneInitializer,
    editor: EditorController,
    desktop_commands: DesktopCommandController,
}

impl TruvisAppClient {
    pub(crate) fn new(ports: TruvisAppClientPorts, initial_scene: InitialScene) -> Self {
        Self {
            scene: SceneInitializer::new(initial_scene),
            editor: EditorController::new(ports.editor, EditorControllerConfig::default()),
            desktop_commands: ports.desktop_commands,
        }
    }
}

impl RendererClient for TruvisAppClient {
    fn initialize(&mut self, world: &mut GameWorld, camera: &mut Camera) {
        self.scene.initialize(world, camera);
    }

    fn tick(&mut self, world: &mut GameWorld, selection: Option<WorldSubmeshSelection>) {
        self.scene.update(world);

        let desktop_update = self.desktop_commands.process_next(world);
        if let Some(scene_version) = desktop_update.scene_version_changed {
            self.editor.notify_scene_version_changed(scene_version);
        }
        self.editor.process_requests(world, selection);
    }

    fn on_selection_changed(&mut self, selection: Option<(WorldSubmeshSelection, MaterialAssetHandle)>) {
        let selection = selection.map(|(selection, material)| (selection.instance, selection.submesh_index, material));
        self.editor.notify_selection_changed(selection);
    }

    fn shutdown(&mut self) {
        self.desktop_commands.shutdown();
        self.editor.shutdown();
    }
}
