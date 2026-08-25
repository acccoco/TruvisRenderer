use std::{fmt::Display, ops::Deref};

/// 帧标签（A/B/C）。
///
/// 表示当前处于 Frames in Flight 的哪一帧。它不是配置项，而是 FIF 资源复用和
/// command buffer / descriptor / per-frame target 选择时使用的运行时索引。
#[derive(Debug, Clone, Copy)]
pub enum FrameLabel {
    A,
    B,
    C,
}

impl Deref for FrameLabel {
    type Target = usize;

    #[inline]
    fn deref(&self) -> &Self::Target {
        match self {
            Self::A => &Self::INDEX[0],
            Self::B => &Self::INDEX[1],
            Self::C => &Self::INDEX[2],
        }
    }
}

impl Display for FrameLabel {
    #[inline]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::A => write!(f, "A"),
            Self::B => write!(f, "B"),
            Self::C => write!(f, "C"),
        }
    }
}

impl FrameLabel {
    pub const COUNT: usize = 3;
    pub const ALL: [Self; Self::COUNT] = [Self::A, Self::B, Self::C];

    const INDEX: [usize; Self::COUNT] = [0, 1, 2];

    #[inline]
    pub fn from_usize(idx: usize) -> Self {
        match idx {
            0 => Self::A,
            1 => Self::B,
            2 => Self::C,
            _ => panic!("Invalid frame index: {idx}"),
        }
    }

    #[inline]
    pub fn from_frame_id(frame_id: u64) -> Self {
        Self::from_usize(frame_id as usize % Self::COUNT)
    }
}
