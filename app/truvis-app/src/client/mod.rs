//! Truvis App 在 RenderThread 上运行的场景与 Editor 业务 Client。

mod desktop_command;
mod editor_controller;
mod world_automation_controller;

use renderer_kit::camera::Camera;
use truvis_editor_bridge::{
    AutomationFrontendEndpoint, AutomationRendererEndpoint, EditorBridgeConfig, EditorFrontendEndpoint,
    EditorRendererEndpoint, create_automation_bridge, create_editor_bridge,
};
use truvis_renderer::RendererClient;
use truvis_renderer::{SceneSelection, SelectionChange};
use truvis_scenes::{InitialScene, SceneInitializer};
use truvis_world::GameWorld;

use self::desktop_command::DesktopCommandController;
use self::editor_controller::{EditorController, EditorControllerConfig};

pub(crate) use self::desktop_command::DesktopCommandSender;

/// App 主线程和 RenderThread 之间的一次性通信装配结果。
///
/// `frontend_editor` 和 sender 留在 Tauri 主线程；`client_ports` 只随 Renderer factory
/// 移入 RenderThread，避免 App 主线程持有 CPU scene 或请求 receiver。
pub(crate) struct TruvisAppWiring {
    pub(crate) frontend_editor: EditorFrontendEndpoint,
    pub(crate) frontend_automation: AutomationFrontendEndpoint,
    pub(crate) client_ports: TruvisAppClientPorts,
    pub(crate) desktop_command_sender: DesktopCommandSender,
}

impl TruvisAppWiring {
    pub(crate) fn new(config: EditorBridgeConfig) -> Self {
        let (frontend_editor, editor) = create_editor_bridge(config);
        let (frontend_automation, automation) = create_automation_bridge(truvis_editor_bridge::EndpointConfig {
            request_capacity: 64,
            notification_capacity: 16,
        });
        let (desktop_command_sender, desktop_commands) = DesktopCommandController::create();
        Self {
            frontend_editor,
            frontend_automation,
            client_ports: TruvisAppClientPorts {
                editor,
                automation,
                desktop_commands,
            },
            desktop_command_sender,
        }
    }
}

/// 只包含 RenderThread 侧 receiver 的 Client 构造输入。
pub(crate) struct TruvisAppClientPorts {
    editor: EditorRendererEndpoint,
    automation: AutomationRendererEndpoint,
    desktop_commands: DesktopCommandController,
}

/// 运行在 RenderThread 上的 Truvis 产品业务 Client。
pub(crate) struct TruvisAppClient {
    scene: SceneInitializer,
    editor: EditorController,
    automation: world_automation_controller::WorldAutomationController,
    desktop_commands: DesktopCommandController,
}

impl TruvisAppClient {
    pub(crate) fn new(ports: TruvisAppClientPorts, initial_scene: InitialScene) -> Self {
        Self {
            scene: SceneInitializer::new(initial_scene),
            editor: EditorController::new(ports.editor, EditorControllerConfig::default()),
            automation: world_automation_controller::WorldAutomationController::new(ports.automation),
            desktop_commands: ports.desktop_commands,
        }
    }
}

impl RendererClient for TruvisAppClient {
    fn initialize(&mut self, world: &mut GameWorld, camera: &mut Camera) {
        self.scene.initialize(world, camera);
    }

    fn tick(&mut self, world: &mut GameWorld, selection: Option<SceneSelection>) {
        self.scene.update(world);

        let desktop_update = self.desktop_commands.process_next(world);
        if let Some(scene_version) = desktop_update.scene_version_changed {
            self.editor.notify_scene_version_changed(scene_version);
        }
        self.editor.process_requests(world, selection);
        self.automation.process_requests(world);
    }

    fn on_selection_changed(&mut self, selection: Option<SelectionChange>) {
        self.editor.notify_selection_changed(selection);
    }

    fn shutdown(&mut self) {
        self.desktop_commands.shutdown();
        self.editor.shutdown();
        self.automation.shutdown();
    }
}
