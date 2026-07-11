use std::fs;
use std::path::PathBuf;

use ts_rs::{Config, TS};

use truvis_editor_bridge::protocol::{EditorClientMessage, EditorServerMessage};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../web/src/protocol/generated");
    fs::create_dir_all(&output_dir)?;
    let config = Config::new().with_out_dir(&output_dir);

    EditorClientMessage::export_all(&config)?;
    EditorServerMessage::export_all(&config)?;

    let exports = [
        "CoverageModeDto",
        "EditorCapabilities",
        "EditorClientMessage",
        "EditorCommand",
        "EditorError",
        "EditorErrorCode",
        "EditorNotification",
        "EditorQuery",
        "EditorRequest",
        "EditorResponse",
        "EditorServerMessage",
        "InstanceId",
        "MaterialClassDto",
        "MaterialDto",
        "MaterialId",
        "MaterialPatch",
        "RequestId",
        "SceneObjectSummary",
        "SceneObjectsPage",
        "SceneVersion",
        "SelectionDto",
        "TextureId",
    ]
    .into_iter()
    .map(|name| format!("export * from './{name}';"))
    .collect::<Vec<_>>()
    .join("\n");
    fs::write(output_dir.join("index.ts"), format!("{exports}\n"))?;

    println!("exported editor TypeScript bindings to {}", output_dir.display());
    Ok(())
}
