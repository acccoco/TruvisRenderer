use std::env;

pub mod common_settings;
pub mod offline_render_graph;
pub mod rt_render_graph;
pub mod targets;

/// App-kit 共享的渲染模式。
///
/// 该枚举只描述 app 选择哪条 sub RenderGraph 出图；实时/离线各自维护资源和 temporal state，
/// 避免 UI 层把 DLSS、ReSTIR 或离线累计状态混在同一份 runtime state 中。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RenderMode {
    #[default]
    Realtime,
    Offline,
}

impl RenderMode {
    pub const ALL: [Self; 2] = [Self::Realtime, Self::Offline];

    pub fn label(self) -> &'static str {
        match self {
            Self::Realtime => "Realtime",
            Self::Offline => "Offline",
        }
    }

    pub fn initial_from_env() -> Self {
        const ENV_NAME: &str = "TRUVIS_RENDER_MODE";
        let Ok(value) = env::var(ENV_NAME) else {
            return Self::Realtime;
        };

        match Self::from_config_value(&value) {
            Some(mode) => {
                log::info!("Initial render mode from {ENV_NAME}={value}: {mode:?}");
                mode
            }
            None => {
                log::warn!("Ignoring unsupported {ENV_NAME} value: {value}");
                Self::Realtime
            }
        }
    }

    fn from_config_value(value: &str) -> Option<Self> {
        let normalized = value.trim().to_ascii_lowercase().replace(['_', '-', ' '], "");
        match normalized.as_str() {
            "realtime" | "real" | "rt" => Some(Self::Realtime),
            "offline" | "groundtruth" | "gt" => Some(Self::Offline),
            _ => None,
        }
    }
}
