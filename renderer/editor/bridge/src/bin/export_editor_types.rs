use std::fs;

use ts_rs::{Config, TS};

use truvis_editor_bridge::protocol::{EditorNotification, EditorRequest, EditorResponse};
use truvis_path::TruvisPath;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output_dir = TruvisPath::workspace_path().join("app/editor/web/src/protocol/generated");
    if output_dir.is_dir() {
        fs::remove_dir_all(&output_dir)?;
    }
    fs::create_dir_all(&output_dir)?;
    let config = Config::new().with_out_dir(&output_dir);

    EditorRequest::export_all(&config)?;
    EditorResponse::export_all(&config)?;
    EditorNotification::export_all(&config)?;

    let exports = [
        "CoverageModeDto",
        "EditorCommand",
        "EditorError",
        "EditorErrorCode",
        "EditorNotification",
        "EditorQuery",
        "EditorRequest",
        "EditorResponse",
        "InstanceDetailsDto",
        "InstanceId",
        "InstanceMaterialBindingDto",
        "MaterialClassDto",
        "MaterialDto",
        "MaterialId",
        "MaterialPatch",
        "MeshId",
        "MeshSummaryDto",
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

    // ts-rs 未启用额外的 formatter feature 时，带字段文档的结构会在类型行末留下空格。
    // 生成器统一清理所有 `.ts` 行尾，保证 codegen 结果可直接通过仓库的 `git diff --check`；
    // 这里只做机械化 whitespace 规范化，不改变任何 TypeScript token 或协议语义。
    for entry in fs::read_dir(&output_dir)? {
        let path = entry?.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("ts") {
            continue;
        }
        let source = fs::read_to_string(&path)?;
        let normalized = source.lines().map(str::trim_end).collect::<Vec<_>>().join("\n");
        fs::write(path, format!("{normalized}\n"))?;
    }

    println!("exported editor TypeScript bindings to {}", output_dir.display());
    Ok(())
}
