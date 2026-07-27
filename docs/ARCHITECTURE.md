# Architecture

## 1. Product shape

RustCut uses a command-driven, non-destructive editing model:

```text
Prompt / UI / MCP
        │
        ▼
Planner (rule or LLM)
        │ EditPlan { commands[] }
        ▼
Command validator + Project mutation
        │
        ├── project.json snapshots (undo / redo)
        ├── transcript timing
        └── editable multi-track Timeline
                      │
                      ▼
               FFmpeg RenderPlan
                      │
                      ▼
                MP4 / FCPXML
```

The planner never emits shell commands. It emits a closed `EditCommand` enum. The core validates and applies commands before the renderer sees the timeline.

## 2. Workspace boundaries

### rustcut-core

Owns all business logic and has no dependency on Axum or MCP:

- domain model;
- project storage;
- media probing/import;
- transcript parsing/transcription adapter;
- rule and LLM planners;
- command application and undo/redo;
- FFmpeg command generation and execution;
- basic FCPXML export.

### rustcut-cli

Thin command-line adapter. It is useful for automation, CI and debugging exact project JSON.

### rustcut-server

Axum HTTP adapter, embedded Web UI, range-based media serving and in-memory render job status. Replace its in-memory jobs with a durable queue before horizontal scaling.

### rustcut-mcp

Local stdio JSON-RPC adapter. It exposes a bounded tool surface and delegates all work to `EditorService`.

## 3. Storage layout

```text
data/
└─ projects/
   └─ <project-uuid>/
      ├─ project.json
      ├─ project.json.bak
      ├─ assets/
      │  └─ <asset-uuid>_<filename>
      ├─ transcripts/
      │  └─ <asset-uuid>.json
      └─ renders/
         └─ render-YYYYMMDD-HHMMSS.mp4
```

`Asset.path` is relative when the file is copied into the project and absolute when `copy=false`.

## 4. Timeline semantics

All time values are integer milliseconds. A media clip has:

- timeline start: `start_ms`;
- source range: `source_in_ms..source_out_ms`;
- playback rate: `speed`;
- calculated timeline duration: `(source_out_ms-source_in_ms)/speed`.

The primary, lowest-order video track is concatenated. Higher video tracks are overlays. Audio tracks are delayed and mixed. Caption and Graphic tracks are rendered as text overlays.

## 5. Undo / redo

A batch of commands is one revision. Before applying the batch, the current timeline is pushed to `undo_stack`; `redo_stack` is cleared. Undo and redo only affect timeline state, not imported source files.

For large production projects, replace whole-timeline snapshots with event sourcing or structural diffs.

## 6. Provider extension points

Implemented traits:

```rust
#[async_trait]
pub trait Planner {
    async fn plan(&self, project: &Project, prompt: &str) -> Result<EditPlan>;
}

#[async_trait]
pub trait Transcriber {
    async fn transcribe(&self, asset_id: Uuid, media_path: &Path) -> Result<Transcript>;
}
```

Recommended additional traits:

- `AssetGenerator` for image/video/music/TTS;
- `SemanticRanker` for highlight selection;
- `ObjectStore` for S3/R2;
- `RenderQueue` for durable worker dispatch;
- `ProjectRepository` for PostgreSQL;
- `RealtimePublisher` for WebSocket/SSE collaboration events.

## 7. Production split

```text
Web / Desktop / Agents
          │
      API gateway + auth
          │
  Project API ─── PostgreSQL
          │
          ├── Object storage / CDN
          ├── Queue
          └── Rust render workers + FFmpeg
                    │
            Transcription / LLM / generation providers
```

Use signed uploads and signed playback URLs. Workers should run with CPU/memory/time limits and no broad host filesystem access.
