use std::fmt;
use std::path::PathBuf;

/// scene edit 中涉及的 handle 类别。
///
/// 该类型只用于错误报告，避免 `SceneStore` 把具体 SlotMap 存储细节暴露到 `World`
/// facade 之外。删除、更新或依赖校验失败时，调用方可以据此判断是哪一类 scene 语义对象失效。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneHandleKind {
    Texture,
    Mesh,
    Material,
    Instance,
    Light,
}

/// `SceneStore` 内部 edit API 的事务失败原因。
///
/// 所有返回该错误的 edit 都必须保持事务语义：不推进 revision、不写入 `SceneChanges`，
/// 也不修改 texture/material/mesh/instance 反向依赖索引。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SceneEditError {
    /// 调用方传入的 SlotMap handle 已失效或不属于当前 scene。
    StaleHandle { kind: SceneHandleKind },
    /// 新对象或更新内容引用了不存在的依赖。
    MissingDependency { kind: SceneHandleKind },
    /// 删除对象仍被其他 scene 对象引用。
    StillReferenced {
        kind: SceneHandleKind,
        dependent_count: usize,
    },
    /// instance 的 material 列表与 mesh submesh 数量不匹配。
    ///
    /// 当前实现还没有在 `SceneStore` 长期保存 submesh 数量，因此该错误先作为目标错误边界保留；
    /// 后续 mesh metadata 落地后由 `register_instance` / `update_instance_materials` 返回。
    MaterialCountMismatch { expected: usize, actual: usize },
}

impl fmt::Display for SceneEditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StaleHandle { kind } => write!(f, "stale scene handle: {kind:?}"),
            Self::MissingDependency { kind } => write!(f, "missing scene dependency: {kind:?}"),
            Self::StillReferenced { kind, dependent_count } => {
                write!(f, "scene {kind:?} is still referenced by {dependent_count} object(s)")
            }
            Self::MaterialCountMismatch { expected, actual } => {
                write!(f, "material count mismatch: expected {expected}, got {actual}")
            }
        }
    }
}

impl std::error::Error for SceneEditError {}

/// `World` facade 对外暴露的 edit 错误。
///
/// `World` 负责补充文件系统 canonicalize、asset ingest 请求等 facade 层失败；真正的
/// scene store 事务失败仍由 `SceneEditError` 保持清晰边界。
#[derive(Debug)]
pub enum WorldEditError {
    Scene(SceneEditError),
    FilesystemCanonicalizeFailed { path: PathBuf, error: String },
}

impl fmt::Display for WorldEditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Scene(err) => err.fmt(f),
            Self::FilesystemCanonicalizeFailed { path, error } => {
                write!(f, "failed to canonicalize path '{}': {error}", path.display())
            }
        }
    }
}

impl std::error::Error for WorldEditError {}

impl From<SceneEditError> for WorldEditError {
    fn from(value: SceneEditError) -> Self {
        Self::Scene(value)
    }
}
