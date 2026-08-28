//! Tauri WebView / Desktop / App 共享的 Editor 协议 DTO。
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
pub use ids::{InstanceId, MaterialId, MeshId, SceneVersion, TextureId};
pub use material::{CoverageModeDto, MaterialClassDto, MaterialDto, MaterialPatch};
pub use message::{EditorCommand, EditorNotification, EditorQuery, EditorRequest, EditorResponse};
pub use scene::{InstanceDetailsDto, InstanceMaterialBindingDto, MeshSummaryDto, SceneObjectSummary, SceneObjectsPage};
pub use selection::SelectionDto;

/// 单页场景对象查询允许的最大对象数。
pub const MAX_SCENE_PAGE_SIZE: u16 = 256;

/// 场景对象查询的默认对象数。
pub const DEFAULT_SCENE_PAGE_SIZE: u16 = 128;
