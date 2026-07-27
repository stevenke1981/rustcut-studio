#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

command -v ffmpeg >/dev/null
command -v ffprobe >/dev/null
cargo build --workspace

PYTHON_CMD=()
if [[ -n "${PYTHON:-}" ]]; then
  PYTHON_CMD=("$PYTHON")
else
  for candidate in python3 python; do
    if command -v "$candidate" >/dev/null && "$candidate" -c "import json" >/dev/null 2>&1; then
      PYTHON_CMD=("$candidate")
      break
    fi
  done
  if [[ ${#PYTHON_CMD[@]} -eq 0 ]] &&
    command -v py >/dev/null &&
    py -3 -c "import json" >/dev/null 2>&1; then
    PYTHON_CMD=(py -3)
  fi
fi
if [[ ${#PYTHON_CMD[@]} -eq 0 ]] ||
  ! "${PYTHON_CMD[@]}" -c "import json" >/dev/null 2>&1; then
  printf 'Python 3 is required (set PYTHON to its executable).\n' >&2
  exit 1
fi

ROOT="data/smoke"
rm -rf "$ROOT"
mkdir -p "$ROOT/input"
ffmpeg -y -hide_banner -loglevel error \
  -f lavfi -i "testsrc2=size=640x360:rate=30:duration=8" \
  -f lavfi -i "sine=frequency=440:sample_rate=48000:duration=8" \
  -c:v libx264 -pix_fmt yuv420p -c:a aac -shortest "$ROOT/input/sample.mp4"
cat > "$ROOT/input/sample.json" <<'JSON'
{
  "language": "zh",
  "segments": [
    {
      "start": 0.5,
      "end": 2.0,
      "text": "嗯 這是 RustCut 測試",
      "words": [
        {"start": 0.5, "end": 0.8, "word": "嗯"},
        {"start": 0.9, "end": 1.2, "word": "這是"},
        {"start": 1.3, "end": 1.7, "word": "RustCut"},
        {"start": 1.7, "end": 2.0, "word": "測試"}
      ]
    },
    {
      "start": 4.0,
      "end": 6.5,
      "text": "我們會移除中間的靜音並加字幕"
    }
  ]
}
JSON

CLI="target/debug/rustcut-cli --data-dir $ROOT/data"
$CLI new "Smoke Test" > "$ROOT/project.json"
PROJECT_ID="$("${PYTHON_CMD[@]}" -c 'import json; print(json.load(open("data/smoke/project.json"))["id"])')"
$CLI import "$PROJECT_ID" "$ROOT/input/sample.mp4" > "$ROOT/import.json"
ASSET_ID="$("${PYTHON_CMD[@]}" -c 'import json; d=json.load(open("data/smoke/import.json")); print(next(iter(d["assets"])))')"
$CLI add "$PROJECT_ID" "$ASSET_ID" > /dev/null
$CLI import-transcript "$PROJECT_ID" "$ASSET_ID" "$ROOT/input/sample.json" > /dev/null
$CLI prompt "$PROJECT_ID" "移除靜音與贅詞，加字幕，改成 9:16" > "$ROOT/prompt.json"
$CLI render "$PROJECT_ID" --output "$ROOT/final.mp4" > "$ROOT/render.json"
ffprobe -v error -show_entries format=duration -of default=nw=1 "$ROOT/final.mp4"
printf 'Smoke test passed: %s\n' "$ROOT/final.mp4"
