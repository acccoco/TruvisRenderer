use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Blender 等离线工具导出的启动场景描述。
///
/// 该类型只描述 CPU 侧场景入口，不包含 Vulkan handle、RenderGraph 或 Renderer 状态。
/// 路径在读取时相对于 manifest 所在目录解析，避免把工作目录规则泄漏到调用方。
#[derive(Clone, Debug, Deserialize)]
pub struct SceneManifest {
    pub name: String,
    pub model: PathBuf,
    pub sky: Option<PathBuf>,
    pub default_camera: String,
    pub cameras: Vec<SceneCamera>,
    #[serde(default)]
    pub area_lights: Vec<SceneAreaLight>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SceneCamera {
    pub name: String,
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    pub fov_deg: f32,
    #[serde(default = "default_near")]
    pub near: f32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SceneAreaLight {
    pub center: [f32; 3],
    pub half_u: [f32; 3],
    pub half_v: [f32; 3],
    pub radiance: [f32; 3],
}

fn default_near() -> f32 {
    0.05
}

impl SceneManifest {
    pub fn load(path: &Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|error| format!("failed to read scene manifest {}: {error}", path.display()))?;
        let mut manifest: Self = serde_json::from_str(&content)
            .map_err(|error| format!("failed to parse scene manifest {}: {error}", path.display()))?;
        let base = path.parent().unwrap_or_else(|| Path::new("."));
        manifest.model = resolve_path(base, manifest.model);
        manifest.sky = manifest.sky.map(|sky| resolve_path(base, sky));
        Ok(manifest)
    }
}

fn resolve_path(base: &Path, path: PathBuf) -> PathBuf {
    if path.is_absolute() { path } else { base.join(path) }
}
