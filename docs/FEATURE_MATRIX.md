# Feature alignment matrix

| Capability | RustCut status | Implementation |
|---|---:|---|
| Prompt-based editing | MVP | Rule planner + optional compatible LLM planner |
| Real editable timeline | MVP | JSON multi-track timeline with command enum |
| Transcript-based cutting | MVP | Segment/word timestamps, silence/filler removal |
| Auto captions | MVP | Transcript to caption clips + FFmpeg drawtext |
| Multi-track video/audio | MVP | Primary concat, overlays and audio mixing |
| Undo / redo | MVP | Persisted timeline snapshots |
| Reframe 16:9 / 9:16 / 1:1 | MVP | Timeline settings + FFmpeg scale/pad |
| Motion graphics | Basic | Editable text/title/lower-third model |
| Noise removal | Extension | Add FFmpeg filter or external provider |
| AI voiceover | Extension | Provider trait recommended |
| AI music | Extension | Provider trait recommended |
| Generate image/video | Extension | Provider trait recommended |
| Stock footage search | Extension | Provider + licensing metadata required |
| Highlight selection | Extension | LLM/embedding ranker required |
| Speaker diarization | Extension | Transcription provider metadata supported |
| Browser editor | Engineering UI | Assets, player, timeline and chat |
| Desktop app | Planned | Tauri 2 shell |
| Agent integration | MVP | Local MCP stdio server |
| XML round-trip | Basic | Primary-track FCPXML 1.11 |
| Collaboration | Planned | Event log + WebSocket + database |
| Cloud render farm | Planned | Queue + isolated workers + object storage |
