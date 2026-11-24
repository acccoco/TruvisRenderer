use std::fmt;

/// scene edit 中涉及的 handle 类别。
///
/// 该类型只用于错误报告，避免 `GameWorld` 把具体 SlotMap 存储细节暴露到 `GameWorld`
/// facade 之外。删除、更新或依赖校验失败时，调用方可以据此判断是哪一类 scene 语义对象失效。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneHandleKind {
    Instance,
    Light,
}

/// `GameWorld` edit API 的校验失败原因。
///
/// 所有返回该错误的 edit 都必须保持原子语义：不推进 revision，
/// 也不修改 实例与灯光记录。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SceneEditError {
    /// 调用方传入的 SlotMap handle 已失效或不属于当前 scene。
    StaleHandle { kind: SceneHandleKind },
    InvalidInstanceData { reason: String },
    InvalidLightData { reason: String },
    InvalidSkyData {
        reason: String,
    },
    /// instance 的 material 列表与 mesh submesh 数量不匹配。
    MaterialCountMismatch { expected: usize, actual: usize },
}

impl fmt::Display for SceneEditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StaleHandle { kind } => write!(f, "stale scene handle: {kind:?}"),
            Self::InvalidInstanceData { reason } => write!(f, "invalid instance data: {reason}"),
            Self::InvalidSkyData { reason } => write!(f, "invalid sky data: {reason}"),
            Self::InvalidLightData { reason } => write!(f, "invalid light data: {reason}"),
            Self::MaterialCountMismatch { expected, actual } => {
                write!(f, "material count mismatch: expected {expected}, got {actual}")
            }
        }
    }
}

impl std::error::Error for SceneEditError {}
