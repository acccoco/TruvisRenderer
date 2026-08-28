//! Tauri 桌面壳到 RenderThread 的进程内特权命令桥。
//!
//! 本模块只传递不能进入通用 Editor DTO 的本地桌面能力。sender 属于 Tauri
//! `TruvisDesktopState`，receiver 由 RenderThread 上的 `DesktopCommandController`
//! 独占；路径不会序列化到 Web，也不会让 Tauri IPC owner 接触 `World`。

use std::path::PathBuf;

use tokio::sync::{mpsc, oneshot};

use truvis_world::{World, WorldEditError};

/// 私有桌面命令队列容量。
///
/// 原生文件对话框同时只允许存在一个，因此这里不需要吸收请求洪峰。容量为一还能让
/// 重复或失控的 WebView invoke 立即得到 busy，而不是积累过期的文件选择结果。
const DESKTOP_COMMAND_CAPACITY: usize = 1;

/// RenderThread 已经把选中文件写入 CPU scene sky 语义状态。
///
/// 该确认不表示 CPU decode、GPU upload 或 importance distribution 已完成；这些阶段仍由
/// 现有异步 asset/render 流程推进。
#[derive(Clone, Copy, Debug)]
pub struct DesktopSkyAccepted;

/// Desktop frontend 可以提交的本机特权命令。
///
/// 此 enum 不进入 `truvis-editor-bridge`，避免把本机绝对路径提升为 WebView Editor 能力。
enum DesktopCommand {
    /// 请求 RenderThread 把本地文件注册为 scene texture 并设为 sky。
    RequestSkyTexture {
        /// Tauri 原生文件对话框返回的本机路径，只在 Rust 进程内传递。
        path: PathBuf,

        /// 向发起该 Tauri command 的 async task 返回 CPU scene 接受结果。
        reply: oneshot::Sender<Result<DesktopSkyAccepted, String>>,
    },
}

/// Tauri `TruvisDesktopState` 持有的命令提交端。
///
/// sender 可以被 async command 临时 clone；它不保存 scene 状态，也不能直接访问
/// `World`。RenderThread receiver 关闭后，所有后续提交必须立即失败。
#[derive(Clone)]
pub struct DesktopCommandSender {
    /// 指向 RenderThread 单消费者队列的有界 sender。
    sender: mpsc::Sender<DesktopCommand>,
}

impl DesktopCommandSender {
    /// 非阻塞提交一次 sky texture 请求，并返回独立 reply receiver。
    ///
    /// 使用 `try_send` 是线程边界不变量：Tauri command 不能等待 RenderThread inbox
    /// 腾出空间，否则 shutdown 或渲染卡顿会把桌面事件循环拖入背压链。
    pub fn try_request_sky_texture(
        &self,
        path: PathBuf,
    ) -> Result<oneshot::Receiver<Result<DesktopSkyAccepted, String>>, String> {
        let (reply, receiver) = oneshot::channel();
        self.sender.try_send(DesktopCommand::RequestSkyTexture { path, reply }).map_err(|error| match error {
            mpsc::error::TrySendError::Full(_) => "HDRI request is already pending".to_string(),
            mpsc::error::TrySendError::Closed(_) => "native renderer is not available".to_string(),
        })?;
        Ok(receiver)
    }
}

/// 单帧处理 desktop command 后需要通知其他 Renderer owner 的窄结果。
///
/// Controller 不直接依赖 `EditorController`；`TruvisRenderer` 负责在同一 update 阶段把
/// scene-version 变化广播给 Tauri WebView。
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct DesktopCommandUpdate {
    /// CPU scene version 确实变化时携带新版本；同路径去重或失败时为 `None`。
    pub(crate) scene_version_changed: Option<u64>,
}

/// RenderThread 独占的桌面命令消费者。
///
/// 每帧最多处理一条命令，并且只有这里可以把本地 `PathBuf` 交给权威 `World`。
/// shutdown 时先关闭 receiver；队列中尚未处理的 command 随后被丢弃，其 oneshot
/// sender 被释放，使等待中的 Tauri command 得到明确的 channel-closed 结果。
pub(crate) struct DesktopCommandController {
    /// Tauri desktop sender 对应的唯一 receiver。
    receiver: mpsc::Receiver<DesktopCommand>,
}

impl DesktopCommandController {
    /// 创建容量固定为一的私有桥，并把 sender / receiver owner 明确分离。
    pub(crate) fn create() -> (DesktopCommandSender, Self) {
        let (sender, receiver) = mpsc::channel(DESKTOP_COMMAND_CAPACITY);
        (DesktopCommandSender { sender }, Self { receiver })
    }

    /// 在 RenderThread update 阶段非阻塞处理至多一条命令。
    pub(crate) fn process_next(&mut self, world: &mut World) -> DesktopCommandUpdate {
        let command = match self.receiver.try_recv() {
            Ok(command) => command,
            Err(mpsc::error::TryRecvError::Empty | mpsc::error::TryRecvError::Disconnected) => {
                return DesktopCommandUpdate::default();
            }
        };

        match command {
            DesktopCommand::RequestSkyTexture { path, reply } => {
                let previous_scene_version = world.scene_view().scene_version();
                let request_result = world
                    .request_sky_texture_from_path(path)
                    .map(|_| DesktopSkyAccepted)
                    .map_err(Self::world_edit_error_message);
                let current_scene_version = world.scene_view().scene_version();
                let scene_version_changed = (request_result.is_ok() && current_scene_version != previous_scene_version)
                    .then_some(current_scene_version);

                // WebView 可能在文件选中后刷新或关闭。reply receiver 消失不能回滚已经提交到
                // CPU scene 的 mutation，因此 send 失败只表示结果无人接收。
                let _ = reply.send(request_result);
                DesktopCommandUpdate { scene_version_changed }
            }
        }
    }

    /// 关闭 receiver 并丢弃未处理命令，使等待 reply 的 Tauri task 在 shutdown 中退出。
    pub(crate) fn shutdown(&mut self) {
        self.receiver.close();
        while self.receiver.try_recv().is_ok() {}
    }

    /// 把 World 错误转换为可展示但不泄露本机完整路径的桌面错误。
    fn world_edit_error_message(error: WorldEditError) -> String {
        match error {
            WorldEditError::FilesystemCanonicalizeFailed { error, .. } => {
                format!("Failed to open the selected HDRI: {error}")
            }
            WorldEditError::Scene(error) => format!("Failed to update the scene sky: {error}"),
        }
    }
}
