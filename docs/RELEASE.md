# Release and packaging

## Local release

```bash
cargo test --workspace
cargo build --release --workspace
./scripts/package.sh
```

The archive includes:

```text
bin/rustcut-cli
bin/rustcut-server
bin/rustcut-mcp
web/
config/
docs/
deploy/
scripts/
README.md
LICENSE
.env.example
.mcp.json.example
```

FFmpeg is not redistributed. Install it separately or place licensed binaries beside the RustCut binaries and set `RUSTCUT_FFMPEG` / `RUSTCUT_FFPROBE`.

## GitHub release

1. Push a tag such as `v0.1.0`.
2. `.github/workflows/release.yml` builds three OS targets.
3. Each runner executes the platform packaging script.
4. The workflow uploads archives to the GitHub Release.

## Container release

```bash
docker build -t rustcut-studio:0.1.0 .
docker run --rm -p 8787:8787 -v "$PWD/data:/data" rustcut-studio:0.1.0
```

## Production checklist

- Put the server behind TLS and authentication.
- Disable permissive CORS.
- Replace path-based import with signed multipart upload.
- Use a database and object storage.
- Use a durable queue and isolated render workers.
- Enforce upload size, project quota, job timeout and codec policies.
- Scan imported files and never trust container metadata.
- Store provider credentials in a secret manager.
- Add audit logs for prompt, plan, command batch and export.
- Add metrics for probe, transcription, planning and render duration/failure.
