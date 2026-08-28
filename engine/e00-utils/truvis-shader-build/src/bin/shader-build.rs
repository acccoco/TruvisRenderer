use std::path::PathBuf;

use truvis_logs::{LogFilePath, TruvisLogger};
use truvis_shader_build::ShaderBuildRunner;
use truvis_shader_manifest::ShaderManifest;

struct CliOptions {
    manifest_path: PathBuf,
    force: bool,
}

impl CliOptions {
    fn parse() -> Result<Self, String> {
        let mut manifest_path = PathBuf::from("shader-packages.toml");
        let mut force = false;
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--force" | "-f" => force = true,
                "--manifest" => {
                    let path = args.next().ok_or_else(|| "--manifest 需要一个路径参数".to_string())?;
                    manifest_path = PathBuf::from(path);
                }
                "--help" | "-h" => {
                    return Err("Usage: shader-build [--manifest <path>] [--force]".to_string());
                }
                _ => return Err(format!("Unsupported shader-build arg '{arg}'")),
            }
        }
        Ok(Self { manifest_path, force })
    }
}

fn main() -> Result<(), String> {
    let options = CliOptions::parse()?;
    let manifest = ShaderManifest::load(&options.manifest_path).map_err(|err| err.to_string())?;
    TruvisLogger::init_with_file(LogFilePath::current_exe(manifest.log_root()));
    ShaderBuildRunner::new(manifest, options.force)?.run()
}
