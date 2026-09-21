use std::ffi::OsString;

use anyhow::{Result, bail};
use truvis_renderer::InitialScene;

/// Truvis 桌面进程启动时的只读选择。
///
/// 参数只影响 RenderThread 创建前选择的初始 CPU scene，不进入 Editor 协议，也不在
/// 运行时修改场景。
pub(crate) struct StartupOptions {
    pub(crate) scene: InitialScene,
    pub(crate) show_help: bool,
}

impl StartupOptions {
    pub(crate) fn from_process_args() -> Result<Self> {
        Self::parse(std::env::args_os().skip(1))
    }

    fn parse<I>(args: I) -> Result<Self>
    where
        I: IntoIterator<Item = OsString>,
    {
        let mut scene = InitialScene::default();
        let mut show_help = false;
        let mut args = args.into_iter();

        while let Some(argument) = args.next() {
            if argument == "--help" || argument == "-h" {
                show_help = true;
                continue;
            }

            if argument == "--scene" {
                let value =
                    args.next().ok_or_else(|| anyhow::anyhow!("missing value for --scene\n\n{}", Self::usage()))?;
                let value = value
                    .to_str()
                    .ok_or_else(|| anyhow::anyhow!("--scene value must be valid UTF-8\n\n{}", Self::usage()))?;
                scene = Self::parse_scene(value)?;
                continue;
            }

            if let Some(value) = argument.to_str().and_then(|value| value.strip_prefix("--scene=")) {
                scene = Self::parse_scene(value)?;
                continue;
            }

            bail!("unknown argument {:?}\n\n{}", argument, Self::usage());
        }

        Ok(Self { scene, show_help })
    }

    fn parse_scene(value: &str) -> Result<InitialScene> {
        InitialScene::parse_name(value).ok_or_else(|| {
            anyhow::anyhow!("unknown scene {:?}; expected one of: manual, sponza\n\n{}", value, Self::usage())
        })
    }

    pub(crate) fn usage() -> &'static str {
        "Usage: truvis-app [--scene manual|sponza]\n\nOptions:\n  --scene <name>  Select the initial scene (default: manual)\n  -h, --help      Show this help"
    }
}
