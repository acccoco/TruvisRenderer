//! Web / Server / App 共享的 Editor 协议 DTO。
//!
//! Rust 类型是协议权威来源；Web 侧 TypeScript 由这些类型生成。所有 World handle 都只以
//! session-local opaque string 表达，协议不会暴露 SlotMap 或 GPU slot 结构。

mod error;
mod ids;
mod material;
mod message;
mod scene;
mod selection;

pub use error::{EditorError, EditorErrorCode};
pub use ids::{ClientId, InstanceId, MaterialId, RequestId, SceneVersion, TextureId};
pub use material::{CoverageModeDto, MaterialClassDto, MaterialDto, MaterialPatch};
pub use message::{
    EditorClientMessage, EditorCommand, EditorNotification, EditorQuery, EditorRequest, EditorResponse,
    EditorServerMessage,
};
pub use scene::{EditorCapabilities, SceneObjectSummary, SceneObjectsPage};
pub use selection::SelectionDto;

/// 第一版 Editor JSON 协议版本。
pub const EDITOR_PROTOCOL_VERSION: u32 = 1;

/// 单页场景对象查询允许的最大对象数。
pub const MAX_SCENE_PAGE_SIZE: u16 = 256;

/// 场景对象查询的默认对象数。
pub const DEFAULT_SCENE_PAGE_SIZE: u16 = 128;
