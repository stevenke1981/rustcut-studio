# RustCut Studio

以 Rust 實作的「對話式 AI 影片剪輯器」參考架構：使用自然語言產生非破壞式剪輯命令，所有變更都落在可編輯、可復原的多軌時間軸，最後交由 FFmpeg 渲染。

> 本專案是獨立實作，沒有使用 ChatCut 的私有程式碼、模型、商標素材或未公開 API。名稱 RustCut Studio 僅用於區分本專案。

## 已完成的功能

- Rust 2024 Cargo workspace：核心函式庫、CLI、Axum REST server、MCP stdio server。
- 專案與素材庫：FFprobe 分析、SHA-256、素材複製、可攜式專案目錄。
- 可編輯多軌時間軸：Video、Audio、Caption、Graphic。
- 非破壞式剪輯命令：加入、修剪、切割、刪除、移動、音量、速度、淡入淡出、重構比例、音量正規化。
- 逐字稿驅動剪輯：匯入 JSON／SRT／VTT、移除靜音、移除贅詞、產生字幕。
- 自然語言規劃：內建中英文規則規劃器；可選 OpenAI-compatible LLM endpoint。
- FFmpeg 輸出：主軌串接、疊加影片、混音、字幕／文字、直式／方形／橫式輸出。
- 瀏覽器工作區：素材、播放器、時間軸、AI 對話三欄介面。
- MCP 工具：建立專案、匯入素材、讀取時間軸、套用提示、復原、輸出、FCPXML。
- 基礎 FCPXML 1.11 匯出。
- Docker、systemd、GitHub Actions、Linux/macOS/Windows 打包腳本、煙霧測試。

## 專案結構

```text
rustcut-studio/
├─ crates/
│  ├─ core/       # 領域模型、剪輯命令、逐字稿、planner、FFmpeg renderer
│  ├─ cli/        # rustcut-cli
│  ├─ server/     # rustcut-server + REST API + Web UI
│  └─ mcp/        # rustcut-mcp，newline-delimited JSON-RPC stdio
├─ web/           # 內嵌式瀏覽器工作區
├─ docs/          # 架構、API、發佈與產品對齊說明
├─ scripts/       # 開發、煙霧測試與跨平台打包
├─ config/
├─ deploy/systemd/
├─ Dockerfile
└─ docker-compose.yml
```

## 系統需求

- Rust `1.97.1`（已由 `rust-toolchain.toml` 固定）。
- FFmpeg 與 FFprobe，兩者需位於 `PATH`，或用環境變數指定。
- 可選：支援 `/chat/completions` 的 LLM endpoint。
- 可選：支援 `/audio/transcriptions` multipart API 的 Whisper-compatible endpoint。

### Ubuntu / Debian

```bash
sudo apt-get update
sudo apt-get install -y ffmpeg build-essential pkg-config
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### macOS

```bash
brew install rust ffmpeg
```

### Windows

1. 安裝 Rustup 與 Visual Studio Build Tools。
2. 安裝 FFmpeg，並將 `ffmpeg.exe`、`ffprobe.exe` 所在目錄加入 `PATH`。
3. 在 PowerShell 執行 `cargo build --release --workspace`。

## 快速開始

```bash
cp .env.example .env
cargo build --workspace
cargo run -p rustcut-server -- --bind 127.0.0.1:8787
```

開啟 `http://127.0.0.1:8787`。

### CLI 範例

```bash
# 1. 建立專案
cargo run -p rustcut-cli -- new "訪談短片"

# 2. 匯入素材；從輸出取得 project_id 與 asset_id
cargo run -p rustcut-cli -- import <PROJECT_ID> ./interview.mp4

# 3. 加到主時間軸
cargo run -p rustcut-cli -- add <PROJECT_ID> <ASSET_ID>

# 4. 匯入逐字稿
cargo run -p rustcut-cli -- import-transcript <PROJECT_ID> <ASSET_ID> ./interview.srt

# 5. 自然語言剪輯
cargo run -p rustcut-cli -- prompt <PROJECT_ID> \
  "移除靜音和贅詞，加上字幕，改成 9:16"

# 6. 先檢查 FFmpeg 命令
cargo run -p rustcut-cli -- render <PROJECT_ID> --dry-run

# 7. 輸出
cargo run -p rustcut-cli -- render <PROJECT_ID> --output ./final.mp4
```

## LLM 規劃器

在 `.env` 設定：

```dotenv
RUSTCUT_LLM_BASE_URL=https://your-provider.example/v1
RUSTCUT_LLM_API_KEY=...
RUSTCUT_LLM_MODEL=your-model
```

使用：

```bash
cargo run -p rustcut-cli -- plan <PROJECT_ID> \
  "剪成節奏快的 45 秒直式精華，保留三個最有力的段落" \
  --planner llm
```

LLM 只產生受限的 `EditCommand` JSON；核心仍會驗證 UUID、時間範圍、速度、音量與尺寸。正式部署時仍應把 plan 先顯示給使用者確認。

## 逐字稿格式

### JSON

```json
{
  "language": "zh",
  "segments": [
    {
      "start": 0.5,
      "end": 3.2,
      "text": "這是一段測試",
      "words": [
        { "start": 0.5, "end": 0.9, "word": "這是" }
      ]
    }
  ]
}
```

也支援專案原生的毫秒欄位：`start_ms`、`end_ms`。

## MCP 設定

先打包或安裝 `rustcut-mcp`，再依 `.mcp.json.example` 設定客戶端：

```json
{
  "mcpServers": {
    "rustcut": {
      "command": "rustcut-mcp",
      "args": ["--data-dir", "./data"]
    }
  }
}
```

MCP server 只在 stdout 寫入 newline-delimited JSON-RPC；診斷訊息不得寫到 stdout。

## REST API

主要端點：

```text
GET    /api/health
GET    /api/projects
POST   /api/projects
GET    /api/projects/{project_id}
POST   /api/projects/{project_id}/assets/import
POST   /api/projects/{project_id}/timeline/add
POST   /api/projects/{project_id}/transcripts/import
POST   /api/projects/{project_id}/plan
POST   /api/projects/{project_id}/prompt
POST   /api/projects/{project_id}/apply
POST   /api/projects/{project_id}/undo
POST   /api/projects/{project_id}/redo
POST   /api/projects/{project_id}/render
GET    /api/jobs/{job_id}
```

完整 request/response 範例見 [`docs/API.md`](docs/API.md)。

本交付包的實際驗證範圍見 [`docs/VALIDATION.md`](docs/VALIDATION.md)。

## 開發與驗證

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
./scripts/smoke-test.sh
```

煙霧測試會用 FFmpeg 產生測試影片、建立逐字稿、套用剪輯並輸出 MP4。

## 打包

### Linux / macOS

```bash
./scripts/package.sh
```

輸出：`dist/rustcut-studio-<version>-<os>-<arch>.tar.gz`。解壓後可執行 `sudo ./scripts/install.sh`，或直接執行 `./scripts/run-server.sh`。

### Windows PowerShell

```powershell
./scripts/package.ps1
```

輸出：`dist/rustcut-studio-<version>-windows-<arch>.zip`。解壓後可執行 `./scripts/install.ps1 -AddToPath`，或直接執行 `./scripts/run-server.ps1`。

## Docker

```bash
docker compose up --build
```

預設把 `./data` 掛載到容器 `/data`。需要匯入容器外的媒體時，請額外把素材目錄以唯讀方式掛載到 `/media`，然後在 Web UI 輸入 `/media/file.mp4`。

## 目前限制

- 瀏覽器介面是可操作的工程版 UI，不是完整 NLE；尚未包含滑鼠拖曳、波形、影格縮圖與 keyframe 編輯器。
- 規則規劃器適合明確命令；「選出最強段落」等語意判斷需要 LLM 或外部 ranking provider。
- 內建字幕以 FFmpeg `drawtext` 渲染；大量逐字字幕會使 filter graph 很長。正式產品可切換到 ASS/libass 或預先合成字幕層。
- FCPXML 匯出目前涵蓋主影片軌，尚未完整映射字幕、混音與效果。
- Web media endpoint 支援基本 byte range，正式公網部署應使用物件儲存與 CDN。
- 沒有內建生成式影片、圖片、音樂與 TTS 模型；這些功能應透過 provider trait 與工作佇列接入。

## 建議的第二階段

1. Tauri 2 桌面殼與原生檔案選擇器。
2. WebGPU 影片預覽、波形與拖曳式 timeline。
3. 本地 Whisper／whisper.cpp provider，含 word-level timestamps 與 speaker diarization。
4. PostgreSQL、S3/R2、分散式 render workers、事件流與多人協作。
5. ASS caption composer、keyframe／transition graph、LUT 與色彩管理。
6. 素材生成 providers：圖片、影片、TTS、音樂、stock search。
7. OTIO、完整 FCPXML、Premiere XML 與 DaVinci Resolve round-trip。

## 安全注意事項

- Server 預設只監聽 `127.0.0.1`。不要直接把無驗證版本暴露到網際網路。
- `/assets/import` 接受 server 本機路徑，適合單機版；SaaS 版必須改成受控上傳、檔案類型驗證、配額與惡意檔案掃描。
- 執行 FFmpeg 時使用結構化參數，不透過 shell；不要把使用者輸入拼接成 shell command。
- LLM plan 必須經過結構化反序列化與核心驗證，不能直接執行任意命令。

## 授權

Apache-2.0。
