use std::fmt;

/// 显式 GPU 资源释放路径携带的类型化原因。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DestroyReason {
    Shutdown,
    Resize,
    DeferredCleanup,
    ImmediateRelease,
    ScopeDrop,
}

impl fmt::Display for DestroyReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Shutdown => write!(f, "shutdown"),
            Self::Resize => write!(f, "resize"),
            Self::DeferredCleanup => write!(f, "deferred-cleanup"),
            Self::ImmediateRelease => write!(f, "immediate-release"),
            Self::ScopeDrop => write!(f, "scope-drop"),
        }
    }
}
