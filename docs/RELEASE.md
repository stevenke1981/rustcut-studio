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

1. Every push to `main` builds a prerelease named `build-<commit SHA>` automatically.
2. Push a tag such as `v0.1.0` to publish a stable release, or manually run Release in Actions.
3. Windows, Linux and macOS run formatting, Clippy, frontend syntax, locked dependency tests and release builds.
4. Publication waits for all three platform packages. Archives and `SHA256SUMS.txt` are uploaded to a draft before publication. Failed builds do not publish a release.
5. Reruns reuse the same commit-based release. Preview releases do not replace the latest stable release.

Packages use the runner's native architecture (Windows/Linux x64 and macOS ARM64).
Download the matching archive from https://github.com/stevenke1981/rustcut-studio/releases.
Install FFmpeg separately. Automatic releases need only the built-in GitHub token; no extra secret is required.

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
