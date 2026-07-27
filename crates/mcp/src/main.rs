use std::{path::PathBuf, sync::Arc};

use anyhow::{Context, Result};
use clap::Parser;
use rustcut_core::{
    EditCommand, EditorService, FsProjectStore, MediaProbe, RenderOptions, Renderer, RulePlanner,
    TimelineSettings,
};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use uuid::Uuid;

#[derive(Debug, Parser)]
#[command(name = "rustcut-mcp", version, about = "RustCut MCP stdio server")]
struct Args {
    #[arg(long, env = "RUSTCUT_DATA_DIR", default_value = "./data")]
    data_dir: PathBuf,

    #[arg(long, env = "RUSTCUT_FFMPEG", default_value = "ffmpeg")]
    ffmpeg: String,

    #[arg(long, env = "RUSTCUT_FFPROBE", default_value = "ffprobe")]
    ffprobe: String,
}

#[derive(Debug, Deserialize)]
struct RpcRequest {
    jsonrpc: String,
    #[serde(default)]
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let args = Args::parse();
    let render_options = RenderOptions {
        ffmpeg: args.ffmpeg,
        ..RenderOptions::default()
    };
    let service = Arc::new(EditorService::new(
        FsProjectStore::new(args.data_dir),
        MediaProbe::new(args.ffprobe),
        Renderer::new(render_options),
    ));
    service.initialize()?;

    let stdin = BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();
    let mut stdout = BufWriter::new(tokio::io::stdout());
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let request: RpcRequest = match serde_json::from_str(&line) {
            Ok(request) => request,
            Err(error) => {
                write_message(
                    &mut stdout,
                    &json!({
                        "jsonrpc": "2.0",
                        "id": null,
                        "error": { "code": -32700, "message": error.to_string() }
                    }),
                )
                .await?;
                continue;
            }
        };
        if request.jsonrpc != "2.0" {
            if let Some(id) = request.id {
                write_message(
                    &mut stdout,
                    &rpc_error(id, -32600, "jsonrpc must equal 2.0"),
                )
                .await?;
            }
            continue;
        }
        let id = request.id.clone();
        let response = handle_request(service.clone(), request).await;
        if let (Some(id), Some(result)) = (id, response) {
            let message = match result {
                Ok(value) => json!({ "jsonrpc": "2.0", "id": id, "result": value }),
                Err(error) => rpc_error(id, -32000, &error.to_string()),
            };
            write_message(&mut stdout, &message).await?;
        }
    }
    Ok(())
}

async fn write_message(writer: &mut BufWriter<tokio::io::Stdout>, value: &Value) -> Result<()> {
    writer
        .write_all(serde_json::to_string(value)?.as_bytes())
        .await?;
    writer.write_all(b"\n").await?;
    writer.flush().await?;
    Ok(())
}

async fn handle_request(service: Arc<EditorService>, request: RpcRequest) -> Option<Result<Value>> {
    match request.method.as_str() {
        "notifications/initialized" | "notifications/cancelled" => None,
        "initialize" => Some(Ok(json!({
            "protocolVersion": "2025-11-25",
            "capabilities": { "tools": { "listChanged": false } },
            "serverInfo": {
                "name": "rustcut-mcp",
                "version": env!("CARGO_PKG_VERSION")
            },
            "instructions": "Create or open a project, import media, place it on the timeline, import or generate a transcript, then apply prompt-driven edits and render."
        }))),
        "ping" => Some(Ok(json!({}))),
        "tools/list" => Some(Ok(json!({ "tools": tool_definitions() }))),
        "tools/call" => Some(handle_tool_call(service, request.params).await),
        _ => request
            .id
            .map(|_| Err(anyhow::anyhow!("method not found: {}", request.method))),
    }
}

async fn handle_tool_call(service: Arc<EditorService>, params: Value) -> Result<Value> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .context("tools/call is missing name")?;
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let value = match name {
        "create_project" => {
            let name = required_string(&arguments, "name")?;
            serde_json::to_value(service.create_project(name, TimelineSettings::default())?)?
        }
        "list_projects" => serde_json::to_value(service.list_projects()?)?,
        "read_project" => {
            let project_id = required_uuid(&arguments, "project_id")?;
            serde_json::to_value(service.get_project(project_id)?)?
        }
        "import_media" => {
            let project_id = required_uuid(&arguments, "project_id")?;
            let path = PathBuf::from(required_string(&arguments, "path")?);
            let copy = arguments
                .get("copy")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            let service_clone = service.clone();
            serde_json::to_value(
                tokio::task::spawn_blocking(move || {
                    service_clone.import_media(project_id, &path, copy)
                })
                .await??,
            )?
        }
        "add_asset_to_timeline" => {
            let project_id = required_uuid(&arguments, "project_id")?;
            let asset_id = required_uuid(&arguments, "asset_id")?;
            let at_ms = arguments.get("at_ms").and_then(Value::as_u64);
            serde_json::to_value(service.apply(
                project_id,
                &[EditCommand::AddAssetToTimeline {
                    asset_id,
                    track_kind: None,
                    at_ms,
                }],
            )?)?
        }
        "import_transcript" => {
            let project_id = required_uuid(&arguments, "project_id")?;
            let asset_id = required_uuid(&arguments, "asset_id")?;
            let path = PathBuf::from(required_string(&arguments, "path")?);
            serde_json::to_value(service.import_transcript(project_id, asset_id, &path)?)?
        }
        "plan_edit" => {
            let project_id = required_uuid(&arguments, "project_id")?;
            let prompt = required_string(&arguments, "prompt")?;
            serde_json::to_value(service.plan_with(project_id, &prompt, &RulePlanner).await?)?
        }
        "apply_prompt" => {
            let project_id = required_uuid(&arguments, "project_id")?;
            let prompt = required_string(&arguments, "prompt")?;
            let auto_apply = arguments
                .get("auto_apply")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            let (plan, project) = service
                .prompt_with(project_id, &prompt, auto_apply, &RulePlanner)
                .await?;
            json!({ "plan": plan, "project": project })
        }
        "undo" => {
            let project_id = required_uuid(&arguments, "project_id")?;
            serde_json::to_value(service.undo(project_id)?)?
        }
        "redo" => {
            let project_id = required_uuid(&arguments, "project_id")?;
            serde_json::to_value(service.redo(project_id)?)?
        }
        "render_project" => {
            let project_id = required_uuid(&arguments, "project_id")?;
            let output = arguments
                .get("output")
                .and_then(Value::as_str)
                .map(PathBuf::from)
                .unwrap_or_else(|| service.default_render_output(project_id));
            let service_clone = service.clone();
            serde_json::to_value(
                tokio::task::spawn_blocking(move || service_clone.render(project_id, output))
                    .await??,
            )?
        }
        "export_fcpxml" => {
            let project_id = required_uuid(&arguments, "project_id")?;
            let output = PathBuf::from(required_string(&arguments, "output")?);
            service.export_fcpxml(project_id, &output)?;
            json!({ "output": output })
        }
        _ => anyhow::bail!("unknown tool: {name}"),
    };

    Ok(json!({
        "content": [{
            "type": "text",
            "text": serde_json::to_string_pretty(&value)?
        }],
        "structuredContent": value,
        "isError": false
    }))
}

fn required_string(value: &Value, key: &str) -> Result<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .with_context(|| format!("missing string argument: {key}"))
}

fn required_uuid(value: &Value, key: &str) -> Result<Uuid> {
    let raw = required_string(value, key)?;
    Uuid::parse_str(&raw).with_context(|| format!("invalid UUID in {key}"))
}

fn rpc_error(id: Value, code: i32, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
    })
}

fn tool_definitions() -> Vec<Value> {
    vec![
        tool(
            "create_project",
            "Create a new editable video project.",
            json!({
                "type": "object", "properties": { "name": { "type": "string" } }, "required": ["name"]
            }),
        ),
        tool(
            "list_projects",
            "List projects.",
            json!({ "type": "object", "properties": {} }),
        ),
        tool(
            "read_project",
            "Read assets and the editable multi-track timeline.",
            project_id_schema(),
        ),
        tool(
            "import_media",
            "Import a local video, audio, or image file.",
            json!({
                "type": "object",
                "properties": {
                    "project_id": { "type": "string", "format": "uuid" },
                    "path": { "type": "string" },
                    "copy": { "type": "boolean", "default": true }
                },
                "required": ["project_id", "path"]
            }),
        ),
        tool(
            "add_asset_to_timeline",
            "Place an imported asset on the timeline.",
            json!({
                "type": "object",
                "properties": {
                    "project_id": { "type": "string", "format": "uuid" },
                    "asset_id": { "type": "string", "format": "uuid" },
                    "at_ms": { "type": "integer", "minimum": 0 }
                },
                "required": ["project_id", "asset_id"]
            }),
        ),
        tool(
            "import_transcript",
            "Import JSON, SRT, or VTT transcript timing.",
            json!({
                "type": "object",
                "properties": {
                    "project_id": { "type": "string", "format": "uuid" },
                    "asset_id": { "type": "string", "format": "uuid" },
                    "path": { "type": "string" }
                },
                "required": ["project_id", "asset_id", "path"]
            }),
        ),
        tool(
            "plan_edit",
            "Turn a natural-language request into a non-destructive edit plan without applying it.",
            prompt_schema(false),
        ),
        tool(
            "apply_prompt",
            "Plan and optionally apply a natural-language edit request.",
            prompt_schema(true),
        ),
        tool(
            "undo",
            "Undo the latest timeline edit.",
            project_id_schema(),
        ),
        tool(
            "redo",
            "Redo the latest undone timeline edit.",
            project_id_schema(),
        ),
        tool(
            "render_project",
            "Render the current timeline to MP4 with FFmpeg.",
            json!({
                "type": "object",
                "properties": {
                    "project_id": { "type": "string", "format": "uuid" },
                    "output": { "type": "string" }
                },
                "required": ["project_id"]
            }),
        ),
        tool(
            "export_fcpxml",
            "Export the primary timeline as FCPXML 1.11.",
            json!({
                "type": "object",
                "properties": {
                    "project_id": { "type": "string", "format": "uuid" },
                    "output": { "type": "string" }
                },
                "required": ["project_id", "output"]
            }),
        ),
    ]
}

fn tool(name: &str, description: &str, input_schema: Value) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": input_schema
    })
}

fn project_id_schema() -> Value {
    json!({
        "type": "object",
        "properties": { "project_id": { "type": "string", "format": "uuid" } },
        "required": ["project_id"]
    })
}

fn prompt_schema(include_apply: bool) -> Value {
    let mut properties = json!({
        "project_id": { "type": "string", "format": "uuid" },
        "prompt": { "type": "string" }
    });
    if include_apply {
        properties.as_object_mut().expect("object").insert(
            "auto_apply".to_string(),
            json!({ "type": "boolean", "default": true }),
        );
    }
    json!({
        "type": "object",
        "properties": properties,
        "required": ["project_id", "prompt"]
    })
}
