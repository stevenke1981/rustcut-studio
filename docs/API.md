# REST API examples

Base URL: `http://127.0.0.1:8787`

## Create project

```http
POST /api/projects
Content-Type: application/json

{
  "name": "訪談短片",
  "width": 1920,
  "height": 1080,
  "fps": 30
}
```

## Import media

The path is resolved on the server machine.

```http
POST /api/projects/{project_id}/assets/import
Content-Type: application/json

{
  "path": "C:\\media\\interview.mp4",
  "copy": true
}
```

## Add an asset to the timeline

```http
POST /api/projects/{project_id}/timeline/add
Content-Type: application/json

{
  "asset_id": "00000000-0000-0000-0000-000000000000",
  "at_ms": 0,
  "track_kind": "video"
}
```

## Import transcript

```http
POST /api/projects/{project_id}/transcripts/import
Content-Type: application/json

{
  "asset_id": "00000000-0000-0000-0000-000000000000",
  "path": "/media/interview.srt"
}
```

## Plan only

```http
POST /api/projects/{project_id}/plan
Content-Type: application/json

{
  "prompt": "移除靜音與贅詞、加字幕、改成直式",
  "planner": "rule",
  "auto_apply": false
}
```

## Plan and apply

```http
POST /api/projects/{project_id}/prompt
Content-Type: application/json

{
  "prompt": "移除靜音與贅詞、加字幕、改成直式",
  "planner": "rule",
  "auto_apply": true
}
```

## Apply explicit commands

```http
POST /api/projects/{project_id}/apply
Content-Type: application/json

{
  "commands": [
    { "type": "reframe", "width": 1080, "height": 1920 },
    {
      "type": "add_text",
      "text": {
        "text": "RustCut",
        "font_file": null,
        "font_size": 96,
        "color": "white",
        "outline_color": "black",
        "outline_width": 4,
        "box_color": null,
        "position": "center",
        "alignment": "center"
      },
      "start_ms": 0,
      "end_ms": 3000,
      "track_id": null
    }
  ]
}
```

## Render

```http
POST /api/projects/{project_id}/render
Content-Type: application/json

{}
```

Response is `202 Accepted` with a job ID. Poll:

```http
GET /api/jobs/{job_id}
```

## Errors

```json
{
  "error": "human-readable message"
}
```

Status codes:

- `400`: invalid command, missing planner configuration, unsupported transcript.
- `404`: project, asset, track, clip or job not found.
- `500`: filesystem, FFmpeg, FFprobe or provider failure.
