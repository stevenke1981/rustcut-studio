#!/usr/bin/env python3
"""Offline structural checks that do not require a Rust toolchain."""
from __future__ import annotations

import json
import os
import pathlib
import shutil
import subprocess
import sys
import tomllib

try:
    import yaml
except ImportError:  # pragma: no cover
    yaml = None

ROOT = pathlib.Path(__file__).resolve().parents[1]
EXCLUDED_DIRS = {".codebase-memory", ".git", "data", "dist", "target"}
REQUIRED = [
    "Cargo.toml",
    "crates/core/src/lib.rs",
    "crates/cli/src/main.rs",
    "crates/server/src/main.rs",
    "crates/mcp/src/main.rs",
    "web/index.html",
    "Dockerfile",
]


def run(command: list[str]) -> None:
    print("+", " ".join(command))
    subprocess.run(command, cwd=ROOT, check=True)


def find_bash() -> str | None:
    configured = os.environ.get("RUSTCUT_BASH")
    candidates = [configured] if configured else []
    if sys.platform == "win32":
        candidates.extend(
            [
                r"C:\Program Files\Git\bin\bash.exe",
                r"C:\Program Files\Git\usr\bin\bash.exe",
            ]
        )
    discovered = shutil.which("bash")
    if discovered:
        candidates.append(discovered)
    return next((candidate for candidate in candidates if candidate and pathlib.Path(candidate).is_file()), None)


def source_files(pattern: str):
    return (
        path
        for path in ROOT.rglob(pattern)
        if not EXCLUDED_DIRS.intersection(path.relative_to(ROOT).parts)
    )


def check_balanced_rust(path: pathlib.Path) -> None:
    source = path.read_text(encoding="utf-8")
    pairs = {"(": ")", "[": "]", "{": "}"}
    stack: list[tuple[str, int]] = []
    quote: str | None = None
    escaped = False
    line_comment = False
    block_depth = 0
    i = 0
    while i < len(source):
        char = source[i]
        nxt = source[i + 1] if i + 1 < len(source) else ""
        if line_comment:
            if char == "\n":
                line_comment = False
            i += 1
            continue
        if block_depth:
            if char == "/" and nxt == "*":
                block_depth += 1
                i += 2
                continue
            if char == "*" and nxt == "/":
                block_depth -= 1
                i += 2
                continue
            i += 1
            continue
        if quote:
            if escaped:
                escaped = False
            elif char == "\\":
                escaped = True
            elif char == quote:
                quote = None
            i += 1
            continue
        if char == "/" and nxt == "/":
            line_comment = True
            i += 2
            continue
        if char == "/" and nxt == "*":
            block_depth = 1
            i += 2
            continue
        if char in ('"', "'"):
            # This is deliberately lightweight; lifetimes may look like quotes.
            if char == "'" and nxt.isalpha():
                i += 1
                continue
            quote = char
            i += 1
            continue
        if char in pairs:
            stack.append((char, i))
        elif char in pairs.values():
            if not stack or pairs[stack[-1][0]] != char:
                raise ValueError(f"unbalanced delimiter in {path} at byte {i}")
            stack.pop()
        i += 1
    if stack or quote or block_depth:
        raise ValueError(f"unterminated structure in {path}")


def main() -> int:
    for relative in REQUIRED:
        if not (ROOT / relative).exists():
            raise FileNotFoundError(relative)

    for path in source_files("*.json"):
        json.loads(path.read_text(encoding="utf-8"))
        print("json ok:", path.relative_to(ROOT))

    for path in source_files("*.toml"):
        tomllib.loads(path.read_text(encoding="utf-8"))
        print("toml ok:", path.relative_to(ROOT))

    if yaml:
        for path in source_files("*.yml"):
            yaml.safe_load(path.read_text(encoding="utf-8"))
            print("yaml ok:", path.relative_to(ROOT))

    for path in source_files("*.rs"):
        check_balanced_rust(path)
        print("rust structure ok:", path.relative_to(ROOT))

    if shutil.which("node"):
        run(["node", "--check", "web/app.js"])
    if bash := find_bash():
        for path in sorted((ROOT / "scripts").glob("*.sh")):
            run([bash, "-n", str(path.relative_to(ROOT))])

    print("Static checks passed. Run cargo fmt/clippy/test for compiler validation.")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as error:
        print(f"static check failed: {error}", file=sys.stderr)
        raise SystemExit(1)
