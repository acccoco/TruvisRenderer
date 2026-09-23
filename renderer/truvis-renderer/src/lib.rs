//! Truvis 产品 Renderer 与 App RenderThread Client 注入边界。

mod overlay_geometry;
mod viewport_overlay;
mod light_overlay;
mod overlay_ui;
mod renderer_client;
mod selection;
mod selection_outline;
mod transform_gizmo;
mod truvis_renderer;

pub use overlay_ui::TruvisOverlayOptions;
pub use renderer_client::RendererClient;
pub use selection::{SceneSelection, SelectionChange};
pub use truvis_renderer::TruvisRenderer;
