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
      "type": "add_captions",
      "asset_id": "ASSET_UUID",
      "preset": "word_pop",
      "style": {
        "font_size": 84,
        "color": "#ffffff",
        "outline_color": "#000000",
        "outline_width": 6,
        "position": "center"
      }
    },
    {
      "type": "add_text",
      "text": {
        "text": "RustCut",
        "font_file": null,
        "font_size": 96,
        "color": "white",
        "opacity": 1.0,
        "outline_color": "black",
        "outline_width": 4,
        "box_color": null,
        "box_padding": 20,
        "shadow_color": null,
        "shadow_x": 3,
        "shadow_y": 3,
        "position": "center",
        "alignment": "center",
        "animation": "pop",
        "animation_duration_ms": 420
      },
      "start_ms": 0,
      "end_ms": 3000,
      "track_id": null
    },
    {
      "type": "add_border",
      "color": "#ffffff",
      "width": 12
    },
    {
      "type": "add_watermark",
      "text": "RustCut Studio",
      "font_file": "C:/Windows/Fonts/msjh.ttc",
      "position": "bottom",
      "font_size": 32,
      "color": "#ffffff",
      "opacity": 0.65,
      "start_ms": 0,
      "end_ms": null
    }
  ]
}
```

`add_captions.preset` 可用值為 `standard`、`hormozi`、`minimal`、`karaoke`
與 `word_pop`。未指定時維持向後相容並使用 `standard`。`karaoke` 與
`word_pop` 會優先使用逐字稿的 word timestamps；若逐字資料不存在則退回逐段字幕。

`animation` 可使用 `none`、`fade`、`slide_up`、`slide_left` 或 `pop`。
`add_border` 會套用至時間軸上的影片片段；`add_watermark` 的 `end_ms: null`
代表持續到時間軸結尾。

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
