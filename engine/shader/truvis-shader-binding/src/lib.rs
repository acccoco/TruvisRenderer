//! 着色器绑定自动生成
//!
//! 通过 `build.rs` 从 `.slangi` 头文件自动生成 Rust 类型绑定。
//! 使用 `bindgen` 解析 Slang 结构体定义，生成与 GPU 内存布局对齐的 Rust 结构体。
//!
//! # 使用示例
//! ```ignore
//! use truvis_shader_binding::gpu::PerFrameData;
//!
//! let data = PerFrameData {
//!     projection: glam::Mat4::IDENTITY.into(),
//!     view: glam::Mat4::IDENTITY.into(),
//!     camera_pos: glam::Vec3::ZERO.into(),
//!     time_ms: 0,
//! };
//! ```

mod _shader_bindings;

pub use crate::_shader_bindings::root as gpu;

mod slang_traits {
    use crate::_shader_bindings::root::*;

    impl From<glam::UVec2> for Uint2 {
        fn from(value: glam::UVec2) -> Self {
            Uint2 { x: value.x, y: value.y }
        }
    }

    impl From<glam::UVec3> for Uint3 {
        fn from(value: glam::UVec3) -> Self {
            Uint3 {
                x: value.x,
                y: value.y,
                z: value.z,
            }
        }
    }

    impl From<glam::UVec4> for Uint4 {
        fn from(value: glam::UVec4) -> Self {
            Uint4 {
                x: value.x,
                y: value.y,
                z: value.z,
                w: value.w,
            }
        }
    }

    impl From<glam::IVec2> for Int2 {
        fn from(value: glam::IVec2) -> Self {
            Int2 { x: value.x, y: value.y }
        }
    }

    impl From<glam::IVec3> for Int3 {
        fn from(value: glam::IVec3) -> Self {
            Int3 {
                x: value.x,
                y: value.y,
                z: value.z,
            }
        }
    }

    impl From<glam::IVec4> for Int4 {
        fn from(value: glam::IVec4) -> Self {
            Int4 {
                x: value.x,
                y: value.y,
                z: value.z,
                w: value.w,
            }
        }
    }

    impl From<glam::Vec2> for Float2 {
        fn from(value: glam::Vec2) -> Self {
            Float2 { x: value.x, y: value.y }
        }
    }

    impl From<glam::Vec3> for Float3 {
        fn from(value: glam::Vec3) -> Self {
            Float3 {
                x: value.x,
                y: value.y,
                z: value.z,
            }
        }
    }

    impl From<glam::Vec4> for Float4 {
        fn from(value: glam::Vec4) -> Self {
            Float4 {
                x: value.x,
                y: value.y,
                z: value.z,
                w: value.w,
            }
        }
    }

    impl From<glam::Mat4> for Float4x4 {
        fn from(value: glam::Mat4) -> Self {
            Float4x4 {
                col0: Float4::from(value.x_axis),
                col1: Float4::from(value.y_axis),
                col2: Float4::from(value.z_axis),
                col3: Float4::from(value.w_axis),
            }
        }
    }
}
