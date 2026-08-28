//! Renderer 层公共组件。
//!
//! 本 crate 只保存生命周期契约、相机/输入和与界面、渲染实现无关的纯 CPU 状态。
//! ImGui、GPU pass 和具体渲染子系统位于各自独立的 Renderer crate，避免基础层反向依赖具体能力。

pub mod camera;
pub mod camera_controller;
pub mod debug_image;
pub mod input_state;
pub mod subsystem;

pub use camera::Camera;
pub use camera_controller::CameraController;
pub use debug_image::{DebugImageOption, DebugImageSelection};
pub use input_state::{InputManager, InputState};
pub use subsystem::{SubsystemLifecycle, SubsystemRenderCtx};
