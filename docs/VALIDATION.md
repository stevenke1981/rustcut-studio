# Validation status

## Windows validation (2026-07-27)

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace` (13 tests passed)
- `python scripts/static-check.py`
- `scripts/smoke-test.sh` through Git Bash

The smoke test created, imported, transcript-edited, and rendered a 1080x1920
H.264/AAC MP4. FFprobe reported a 4.36-second output.

## Completed in the delivery environment

- Parsed every JSON and TOML file.
- Parsed GitHub Actions YAML.
- Checked JavaScript syntax with Node.js.
- Checked every shell script with `bash -n`.
- Performed a lightweight delimiter/string/comment structural scan of every Rust source file.
- Executed a representative FFmpeg filter graph using generated video and audio. The result contained H.264 video, AAC audio, a text layer, the requested resolution, and the requested duration.

## Historical delivery-environment limitation

The original delivery environment could not resolve or download the Rust
distribution server and did not already contain `rustc` or `cargo`. The
compiler-backed checks listed above were subsequently completed on Windows.

Run the following after installing the pinned toolchain:

```bash
python3 scripts/static-check.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
./scripts/smoke-test.sh
```

The included GitHub Actions CI runs the compiler-backed checks on Ubuntu, macOS, and Windows; the Unix runners also execute the end-to-end FFmpeg smoke test.
